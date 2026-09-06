import { test, expect } from "@playwright/test";
import { DEFAULT_SETTINGS } from "../src/lib/types";

// Exercise the production bundle, with synthetic IPC instead of microphones,
// credentials, paid API calls, or the user's real clipboard/history.
test.beforeEach(async ({ page }) => {
  await page.addInitScript(({ settings }) => {
    const latest = { id: "2", title: "A focused tool for clear thoughts", text: "A focused tool for clear thoughts.\n\nKeep the interface quiet, the recording fluid, and the words close at hand.", durationMs: 18000, engine: "open_ai" };
    const older = { ...latest, id: "1", title: "Ideas for the next release", text: "A previous thought, safely kept on this device." };
    let history = [latest, older];
    let started = 0;
    let counter = 0;
    const callbacks = new Map<number, (event: unknown) => void>();
    Object.assign(window, {
      isTauri: true,
      __copiedText: "",
      __TAURI_EVENT_PLUGIN_INTERNALS__: { unregisterListener: () => {} },
      __TAURI_INTERNALS__: {
        transformCallback: (callback: (event: unknown) => void) => { callbacks.set(++counter, callback); return counter; },
        invoke: async (command: string, args: Record<string, unknown>) => {
          switch (command) {
            case "get_settings": return settings;
            case "save_settings": return;
            case "has_openai_api_key": return true;
            case "list_input_devices": case "list_local_models": return [];
            case "list_history": return history;
            case "clear_history": history = []; return;
            case "copy_text": Object.assign(window, { __copiedText: args.text }); return;
            case "start_recording": started = Date.now(); return;
            case "cancel_recording": started = 0; return;
            case "get_recording_status": return { recording: !!started, elapsedSeconds: Math.floor((Date.now() - started) / 1000), limitReached: false, level: .45 + Math.sin(Date.now() / 280) * .3 };
            case "finish_recording": started = 0; return { ...latest, historyEntry: latest, savedPath: null, deliveryWarnings: [] };
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
  await page.getByRole("option", { name: "Ideas for the next release" }).click();
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
});
