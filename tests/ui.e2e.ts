import { test, expect } from "@playwright/test";
import { DEFAULT_SETTINGS } from "../src/lib/types";

// Exercise the production bundle, with synthetic IPC instead of microphones,
// credentials, paid API calls, or the user's real clipboard/history.
test.beforeEach(async ({ page }) => {
  await page.addInitScript(({ settings }) => {
    const latest = { id: "2", createdAtMs: Date.now() - 5 * 60_000, title: "A focused tool for clear thoughts", text: "A focused tool for clear thoughts.\n\nKeep the interface quiet, the recording fluid, and the words close at hand.", durationMs: 18000, engine: "open_ai" };
    const older = { ...latest, createdAtMs: new Date(2026, 0, 2, 9, 15).getTime(), id: "1", title: "Ideas for the next release", text: "A previous thought, safely kept on this device." };
    let history = [latest, older];
    let started = 0;
    let pausedAt = 0;
    let pausedMs = 0;
    const recordingStatus = () => ({ recording: !!started, paused: !!pausedAt, elapsedSeconds: Math.floor(((pausedAt || Date.now()) - started - pausedMs) / 1000), limitReached: false, level: pausedAt ? 0 : .45 + Math.sin(Date.now() / 280) * .3 });
    const models = [
      { id: "tiny", name: "Whisper Tiny", description: "Fastest · suitable for quick drafts", sizeBytes: 77691713, downloaded: false },
      { id: "base", name: "Whisper Base", description: "Balanced · recommended for most devices", sizeBytes: 147951465, downloaded: true },
      { id: "small", name: "Whisper Small", description: "More accurate · requires more memory and time", sizeBytes: 487601967, downloaded: false },
    ];
    let counter = 0;
    const callbacks = new Map<number, (event: unknown) => void>();
    Object.assign(window, {
      isTauri: true,
      __copiedText: "",
      __finishCount: 0,
      __savedSettings: null,
      __TAURI_EVENT_PLUGIN_INTERNALS__: { unregisterListener: () => {} },
      __TAURI_INTERNALS__: {
        transformCallback: (callback: (event: unknown) => void) => { callbacks.set(++counter, callback); return counter; },
        invoke: async (command: string, args: Record<string, unknown>) => {
          switch (command) {
            case "get_settings": return settings;
            case "save_settings": Object.assign(window, { __savedSettings: args.value }); return;
            case "has_openai_api_key": return true;
            case "list_input_devices": return [{ id: "test-mic", name: "Studio microphone", isDefault: true }];
            case "list_local_models": return models;
            case "download_local_model": case "delete_local_model": {
              const model = models.find((model) => model.id === args.modelId);
              if (model) model.downloaded = command === "download_local_model";
              return;
            }
            case "list_history": return history;
            case "clear_history": history = []; return;
            case "copy_text": Object.assign(window, { __copiedText: args.text }); return;
            case "start_recording": started = Date.now(); pausedAt = 0; pausedMs = 0; return;
            case "set_recording_paused":
              if (args.paused && !pausedAt) pausedAt = Date.now();
              else if (!args.paused && pausedAt) { pausedMs += Date.now() - pausedAt; pausedAt = 0; }
              return recordingStatus();
            case "cancel_recording": started = 0; pausedAt = 0; return;
            case "get_recording_status": return recordingStatus();
            case "finish_recording": started = 0; Object.assign(window, { __finishCount: Reflect.get(window, "__finishCount") + 1 }); return { ...latest, historyEntry: latest, savedPath: null, deliveryWarnings: [] };
            case "plugin:event|listen": return ++counter;
            case "plugin:event|unlisten": return;
            default: throw new Error(`Unexpected IPC command: ${command}`);
          }
        },
      },
    });
  }, { settings: { ...DEFAULT_SETTINGS, theme: "dark" } });
});

test("production UI records, restores history, and keeps themed menus usable", async ({ page }, testInfo) => {
  const errors: string[] = [];
  page.on("pageerror", (error) => errors.push(error.message));
  await page.goto("/");
  await expect(page.getByRole("region", { name: "Transcript", exact: true })).toContainText("A focused tool");
  await page.getByRole("combobox", { name: "Action", exact: true }).click();
  await page.waitForTimeout(200); // Let the menu's entry transition settle for the visual artifact.
  await page.screenshot({ path: testInfo.outputPath("action-menu.png") });
  await page.getByRole("option", { name: /Polish/ }).click();
  await page.getByRole("button", { name: "Start recording", exact: true }).click();
  await expect(page.locator("main")).toHaveClass(/recording/);
  await expect(page.locator(".timer")).toContainText("00:01");
  await expect(page.getByRole("region", { name: "Transcript", exact: true })).toBeVisible();
  await page.screenshot({ path: testInfo.outputPath("recording-dark.png") });
  await page.evaluate(() => document.documentElement.dataset.theme = "light");
  await page.waitForTimeout(350);
  await page.screenshot({ path: testInfo.outputPath("recording-light.png") });
  await page.getByRole("button", { name: "Stop recording", exact: true }).click();
  await expect(page.getByText("Copied to clipboard", { exact: true })).toBeVisible();
  await page.getByRole("combobox", { name: "Recent texts" }).click();
  await page.waitForTimeout(350);
  await page.screenshot({ path: testInfo.outputPath("history-menu.png") });
  await page.getByRole("option", { name: /Ideas for the next release/ }).click();
  await page.keyboard.press("Control+Shift+C");
  await expect.poll(() => page.evaluate(() => Reflect.get(window, "__copiedText"))).toContain("A previous thought");
  await page.getByRole("button", { name: "File F", exact: true }).click();
  await page.getByRole("combobox", { name: "File format" }).click();
  await page.getByRole("option", { name: /Markdown/ }).click();
  await expect(page.getByRole("combobox", { name: "File format" })).toContainText("Markdown");
  const historyBox = await page.getByRole("combobox", { name: "Recent texts" }).boundingBox();
  expect(historyBox!.y + historyBox!.height).toBeLessThanOrEqual(720);
  expect(errors).toEqual([]);
});

test("pause freezes recording time and Space finishes even with Pause focused", async ({ page }, testInfo) => {
  await page.goto("/");
  await page.getByRole("button", { name: "Start recording", exact: true }).click();
  await expect(page.locator(".timer")).toContainText("00:01");
  await page.getByRole("button", { name: "Pause recording", exact: true }).click();
  await expect(page.locator("main")).toHaveClass(/paused/);
  const time = await page.locator(".timer").textContent();
  await page.waitForTimeout(1200);
  expect(await page.locator(".timer").textContent()).toBe(time);
  expect(await page.evaluate(() => Reflect.get(window, "__finishCount"))).toBe(0);
  await expect(page.getByRole("button", { name: "Open settings" })).toBeDisabled();
  await page.screenshot({ path: testInfo.outputPath("paused-dark.png") });
  await page.keyboard.press("p");
  await expect(page.locator("main")).toHaveClass(/recording/);
  await expect(page.locator(".timer")).not.toHaveText(time!);
  await page.keyboard.press("p");
  await expect(page.locator("main")).toHaveClass(/paused/);
  await page.getByRole("button", { name: "Resume recording" }).press("Space");
  await expect(page.getByText("Copied to clipboard", { exact: true })).toBeVisible();
  expect(await page.evaluate(() => Reflect.get(window, "__finishCount"))).toBe(1);
});

test("settings share branding, themed model controls and a keyboard-safe dialog", async ({ page }, testInfo) => {
  await page.goto("/");
  const trigger = page.getByRole("button", { name: "Open settings" });
  const icon = await page.locator(".brand-mark").getAttribute("src");
  await trigger.click();
  await expect(page.getByRole("dialog", { name: "Settings" })).toBeVisible();
  await expect(page.locator(".settings-brand img")).toHaveAttribute("src", icon!);
  await expect(page.getByRole("button", { name: "Close settings" })).toBeFocused();
  await page.keyboard.press("Shift+Tab");
  await expect(page.getByRole("button", { name: "Save settings" })).toBeFocused();
  await page.keyboard.press("Tab");
  await expect(page.getByRole("button", { name: "Close settings" })).toBeFocused();
  await page.waitForTimeout(350);
  await page.screenshot({ path: testInfo.outputPath("settings-dark.png") });
  await page.locator(".local-models").scrollIntoViewIfNeeded();
  await expect(page.locator(".model-row.selected")).toContainText("Whisper Base");
  await page.screenshot({ path: testInfo.outputPath("models-dark.png") });
  await page.getByRole("combobox", { name: "Selected local model" }).click();
  await expect(page.getByRole("listbox", { name: "Selected local model" })).toBeVisible();
  await page.waitForTimeout(200);
  const menuBox = await page.getByRole("listbox", { name: "Selected local model" }).boundingBox();
  const scrollBox = await page.locator(".settings-scroll").boundingBox();
  expect(menuBox!.y).toBeGreaterThanOrEqual(scrollBox!.y);
  expect(menuBox!.y + menuBox!.height).toBeLessThanOrEqual(scrollBox!.y + scrollBox!.height);
  await page.screenshot({ path: testInfo.outputPath("models-menu-dark.png") });
  await page.keyboard.press("Escape");
  await expect(page.getByRole("dialog")).toBeVisible();
  await page.keyboard.press("Escape");
  await expect(page.getByRole("dialog")).toHaveCount(0);
  await expect(trigger).toBeFocused();
  await trigger.click();
  await page.getByRole("button", { name: "Light", exact: true }).click();
  await page.waitForTimeout(350);
  await page.screenshot({ path: testInfo.outputPath("settings-light.png") });
  await page.getByRole("combobox", { name: "Microphone", exact: true }).click();
  await page.getByRole("option", { name: /Studio microphone/ }).click();
  await page.locator(".local-models").scrollIntoViewIfNeeded();
  await page.getByRole("button", { name: "Download Whisper Tiny", exact: true }).click();
  await expect(page.getByRole("button", { name: "Remove Whisper Tiny", exact: true })).toBeVisible();
  await page.getByRole("combobox", { name: "Selected local model" }).click();
  await page.getByRole("option", { name: /Whisper Tiny/ }).click();
  await expect(page.locator(".model-row.selected")).toContainText("Whisper Tiny");
  await page.waitForTimeout(250);
  await page.screenshot({ path: testInfo.outputPath("models-light.png") });
  await page.getByRole("button", { name: "Save settings" }).click();
  expect(await page.evaluate(() => Reflect.get(window, "__savedSettings"))).toMatchObject({ theme: "light", local_model_id: "tiny", input_device: "test-mic" });
});

test("history shows local European dates in both the result and menu", async ({ page }) => {
  await page.goto("/");
  await expect(page.locator(".result-date")).toHaveText(/Today · \d{2}:\d{2} · 5 min ago/);
  await page.getByRole("combobox", { name: "Recent texts" }).click();
  await expect(page.getByRole("option", { name: /Ideas for the next release/ })).toContainText("02.01.2026");
  await page.getByRole("option", { name: /Ideas for the next release/ }).click();
  await expect(page.locator(".result-date")).toHaveText("02.01.2026");
  await expect(page.locator(".result-date")).toHaveAttribute("title", "02.01.2026 · 09:15");
});

test("compact layout and reduced motion preserve readable controls", async ({ page }, testInfo) => {
  await page.setViewportSize({ width: 720, height: 620 });
  await page.emulateMedia({ reducedMotion: "reduce" });
  await page.goto("/");
  await page.getByRole("button", { name: "Start recording", exact: true }).click();
  await expect(page.locator("main")).toHaveClass(/recording/);
  expect(await page.locator(".aurora-one").evaluate((el) => getComputedStyle(el).animationName)).toBe("none");
  expect(await page.evaluate(() => document.documentElement.scrollWidth <= window.innerWidth)).toBe(true);
  await page.screenshot({ path: testInfo.outputPath("compact-reduced-motion.png"), fullPage: true });
  await page.getByRole("button", { name: "Stop recording", exact: true }).press("Escape");
  await expect(page.getByText("Recording discarded", { exact: true })).toBeVisible();
  await page.getByRole("button", { name: "Open settings" }).click();
  expect(await page.locator(".settings-modal").evaluate((el) => getComputedStyle(el).animationName)).toBe("none");
  await page.locator(".local-models").scrollIntoViewIfNeeded();
  await page.screenshot({ path: testInfo.outputPath("models-compact-reduced-motion.png") });
});
