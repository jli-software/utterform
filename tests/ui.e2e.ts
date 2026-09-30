import { test, expect, type Page } from "@playwright/test";
import { DEFAULT_SETTINGS } from "../src/lib/types";

const ribbonPixels = (page: Page) => page.locator<HTMLCanvasElement>(".signal-field").evaluate((canvas) => canvas.toDataURL());

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
    let autostart = false;
    const eventHandlers = new Map<string, number>();
    const callbacks = new Map<number, (event: unknown) => void>();
    Object.assign(window, {
      isTauri: true,
      __copiedText: "",
      __emitRemoteIntent: (intent: string) => callbacks.get(eventHandlers.get("remote-intent") ?? -1)?.({ payload: intent }),
      __finishCount: 0,
      __savedSettings: null,
      __appliedHotkey: undefined,
      // What the operating system's startup entry would hold, and every write
      // the interface asked for, in order.
      __autostartWrites: [] as string[],
      // Support actions and frontend failure reports, in the order they arrived.
      __supportCalls: [] as string[],
      __failSupport: "",
      __frontendReports: [] as unknown[],
      __TAURI_EVENT_PLUGIN_INTERNALS__: { unregisterListener: () => {} },
      __TAURI_INTERNALS__: {
        transformCallback: (callback: (event: unknown) => void) => { callbacks.set(++counter, callback); return counter; },
        invoke: async (command: string, args: Record<string, unknown>) => {
          switch (command) {
            case "take_startup_intent": return null;
            case "live_support": return { supported: true, explanation: "" };
            case "typing_support": return { supported: true, explanation: "" };
            case "get_live_status": return { text: "Live words at the cursor", insertedText: "Live words ", deliveryPaused: true, warning: "Focus changed. Insertion is paused.", phase: "streaming" };
            case "global_hotkey_support": return { supported: true, default: "Ctrl+Alt+D", explanation: "", failure: null };
            case "apply_global_hotkey": Object.assign(window, { __appliedHotkey: args.shortcut }); return;
            case "autostart_enabled": return autostart;
            case "enable_autostart": case "disable_autostart":
              autostart = command === "enable_autostart";
              (Reflect.get(window, "__autostartWrites") as string[]).push(command.split("_")[0]);
              return;
            case "list_built_in_actions": return [
              { id: "plain", name: "Plain", hint: "Transcription only", prompt: "" },
              { id: "clean", name: "Clean", hint: "Fix punctuation and obvious errors", prompt: "Correct punctuation, capitalization, spelling, and paragraph breaks. Return only the corrected text." },
              { id: "polish", name: "Polish", hint: "Rewrite for clarity", prompt: "Rewrite the transcript into clear, fluent prose in the speaker's language. Return only the polished text." },
              { id: "summarize", name: "Summarize", hint: "Keep the essentials", prompt: "Summarize the transcript concisely in the speaker's language. Return only the summary." },
              { id: "prompt", name: "Prompt", hint: "Shape it into an AI prompt", prompt: "Convert the transcript into a precise, self-contained prompt for an AI assistant. Return only the prompt." },
              { id: "email", name: "Email", hint: "Turn it into a ready-to-send email", prompt: "Write the transcript as an email in the speaker's language. Return only the email." },
            ];
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
            case "finish_recording": started = 0; Object.assign(window, { __finishCount: Reflect.get(window, "__finishCount") + 1 }); return { ...latest, historyEntry: latest, savedPath: null, copiedToClipboard: true, deliveryWarnings: [] };
            case "play_test_cues": return;
            case "diagnostics_info": return { logPath: "/home/tester/.local/share/software.jli.utterform/logs/utterform.log" };
            case "copy_diagnostics": case "open_log_file": case "open_log_folder":
              (Reflect.get(window, "__supportCalls") as string[]).push(command);
              if (Reflect.get(window, "__failSupport") === command) throw "The log folder could not be opened (ref 0a1b2c3d-4)";
              return command === "copy_diagnostics" ? { bytes: 12698, lines: 318, truncated: false } : undefined;
            case "report_frontend_error":
              (Reflect.get(window, "__frontendReports") as unknown[]).push(args.report);
              return "0a1b2c3d-9";
            case "plugin:event|listen": eventHandlers.set(args.event as string, args.handler as number); return ++counter;
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
  await page.getByRole("button", { name: /Latest text/ }).click();
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
  await page.getByRole("button", { name: "File", exact: true }).click();
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
  const movingContour = await ribbonPixels(page);
  await expect.poll(() => ribbonPixels(page)).not.toBe(movingContour);
  await page.getByRole("button", { name: "Pause recording", exact: true }).click();
  await expect(page.locator("main")).toHaveClass(/paused/);
  const time = await page.locator(".timer").textContent();
  const pausedContour = await ribbonPixels(page);
  await page.waitForTimeout(1200);
  expect(await page.locator(".timer").textContent()).toBe(time);
  expect(await ribbonPixels(page)).toBe(pausedContour);
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
  const icon = await page.locator(".brand-mark").innerHTML();
  await trigger.click();
  await expect(page.getByRole("dialog", { name: "Settings" })).toBeVisible();
  expect(await page.locator(".settings-mark").innerHTML()).toBe(icon);
  await expect(page.getByRole("button", { name: "Close settings" })).toBeFocused();
  await page.keyboard.press("Shift+Tab");
  await expect(page.getByRole("button", { name: "Save settings" })).toBeFocused();
  await page.keyboard.press("Tab");
  await expect(page.getByRole("button", { name: "Close settings" })).toBeFocused();
  await page.waitForTimeout(350);
  await page.screenshot({ path: testInfo.outputPath("settings-dark.png") });
  await page.getByRole("tab", { name: "AI & Models", exact: true }).click();
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
  await page.getByRole("tab", { name: /General/ }).click();
  await page.getByRole("button", { name: "Light", exact: true }).click();
  await page.getByRole("tab", { name: /Recording/ }).click();
  await page.waitForTimeout(350);
  await page.screenshot({ path: testInfo.outputPath("settings-light.png") });
  await page.getByRole("combobox", { name: "Microphone", exact: true }).click();
  await page.getByRole("option", { name: /Studio microphone/ }).click();
  await page.getByRole("tab", { name: "AI & Models", exact: true }).click();
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

test("prompts can be rewritten, read against the original, and reset", async ({ page }, testInfo) => {
  await page.goto("/");
  await page.getByRole("button", { name: "Open settings" }).click();
  await page.getByRole("tab", { name: /Actions/ }).click();
  await page.getByRole("button", { name: /^Email/ }).click();
  const editor = page.getByRole("textbox", { name: "Prompt instructions" });
  await expect(editor).toHaveValue(/Write the transcript as an email/);
  await page.waitForTimeout(250);
  await page.screenshot({ path: testInfo.outputPath("prompts-dark.png") });

  await editor.fill("Answer in two short paragraphs, signed with my first name.");
  await page.getByRole("button", { name: "View the original" }).click();
  await expect(page.locator(".prompt-default")).toContainText("Write the transcript as an email");
  await page.getByRole("textbox", { name: "Prompt name" }).fill("Reply");
  await expect(page.getByRole("button", { name: /^Reply/ })).toBeVisible();
  await page.screenshot({ path: testInfo.outputPath("prompts-edited-dark.png") });

  await page.getByRole("button", { name: "Save settings" }).click();
  expect(await page.evaluate(() => Reflect.get(window, "__savedSettings"))).toMatchObject({
    action_overrides: { email: { name: "Reply", prompt: "Answer in two short paragraphs, signed with my first name." } },
  });

  // The renamed action is what the main window now offers.
  await page.getByRole("combobox", { name: "Action", exact: true }).click();
  await expect(page.getByRole("option", { name: /Reply/ })).toBeVisible();
  await page.keyboard.press("Escape");

  await page.getByRole("button", { name: "Open settings" }).click();
  await page.getByRole("tab", { name: /Actions/ }).click();
  await page.getByRole("button", { name: /^Reply/ }).click();
  await page.getByRole("button", { name: "Reset" }).click();
  await expect(editor).toHaveValue(/Write the transcript as an email/);
  await page.getByRole("button", { name: "Save settings" }).click();
  expect(await page.evaluate(() => Reflect.get(window, "__savedSettings"))).toMatchObject({ action_overrides: {} });
});

test("vocabulary and effort are set where the recording is configured", async ({ page }, testInfo) => {
  await page.goto("/");
  await page.getByRole("button", { name: "Open settings" }).click();
  await page.getByRole("tab", { name: "Recording", exact: true }).click();
  await page.getByRole("textbox", { name: "Vocabulary" }).fill("Careum\nUtterform\n<tagged>");
  await expect(page.getByRole("alert")).toContainText("<tagged>");
  await page.waitForTimeout(200);
  await page.screenshot({ path: testInfo.outputPath("vocabulary-dark.png") });

  await page.getByRole("textbox", { name: "Vocabulary" }).fill("Careum\nUtterform");
  await expect(page.getByRole("alert")).toHaveCount(0);
  await page.getByRole("tab", { name: "AI & Models", exact: true }).click();
  await page.getByRole("button", { name: "Low", exact: true }).click();
  await page.getByRole("button", { name: "Save settings" }).click();
  expect(await page.evaluate(() => Reflect.get(window, "__savedSettings"))).toMatchObject({
    vocabulary: ["Careum", "Utterform"], text_effort: "low",
  });
});

test("history shows local European dates in both the result and menu", async ({ page }) => {
  await page.goto("/");
  await page.getByRole("button", { name: /Latest text/ }).click();
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
  const contour = await ribbonPixels(page);
  await page.waitForTimeout(160);
  expect(await ribbonPixels(page)).toBe(contour);
  expect(await page.evaluate(() => document.documentElement.scrollWidth <= window.innerWidth)).toBe(true);
  await page.screenshot({ path: testInfo.outputPath("compact-reduced-motion.png"), fullPage: true });
  await page.getByRole("button", { name: "Stop recording", exact: true }).press("Escape");
  await expect(page.getByText("Recording discarded", { exact: true })).toBeVisible();
  await page.getByRole("button", { name: "Open settings" }).click();
  expect(await page.locator(".settings-modal").evaluate((el) => getComputedStyle(el).animationName)).toBe("none");
  await page.getByRole("tab", { name: "AI & Models", exact: true }).click();
  await page.locator(".local-models").scrollIntoViewIfNeeded();
  await page.screenshot({ path: testInfo.outputPath("models-compact-reduced-motion.png") });

  // The shortcut recorder and the startup switch have to stay reachable,
  // focusable and uncut where the dialog has least room.
  const insideTheScroll = async (control: ReturnType<typeof page.getByRole>) => {
    await control.scrollIntoViewIfNeeded();
    await expect(control).toBeVisible();
    await control.focus();
    await expect(control).toBeFocused();
    const scroll = (await page.locator(".settings-scroll").boundingBox())!;
    const box = (await control.boundingBox())!;
    expect(box.y).toBeGreaterThanOrEqual(scroll.y - 1);
    expect(box.y + box.height).toBeLessThanOrEqual(scroll.y + scroll.height + 1);
    expect(box.x + box.width).toBeLessThanOrEqual(scroll.x + scroll.width + 1);
  };
  await page.getByRole("tab", { name: /General/ }).click();
  await insideTheScroll(page.getByRole("button", { name: /Shortcut/ }));
  await page.getByRole("tab", { name: /General/ }).click();
  await insideTheScroll(page.getByRole("checkbox", { name: /when I sign in/ }));
  await page.screenshot({ path: testInfo.outputPath("startup-compact.png") });
});


for (const viewport of [{ width: 360, height: 400 }, { width: 480, height: 480 }, { width: 920, height: 400 }]) {
  test(`floating recorder stays in view at ${viewport.width}×${viewport.height}`, async ({ page }, testInfo) => {
    await page.setViewportSize(viewport);
    await page.goto("/");
    const disclosure = page.getByRole("button", { name: /Latest text/ });
    await expect(disclosure).toHaveAttribute("aria-expanded", "false");
    await expect(page.getByRole("region", { name: "Transcript", exact: true })).toHaveCount(0);
    const inView = async (selector: string) => {
      const box = await page.locator(selector).boundingBox();
      expect(box).not.toBeNull();
      expect(box!.x).toBeGreaterThanOrEqual(0);
      expect(box!.y).toBeGreaterThanOrEqual(0);
      expect(box!.x + box!.width).toBeLessThanOrEqual(viewport.width);
      expect(box!.y + box!.height).toBeLessThanOrEqual(viewport.height);
    };
    await inView(".copy-button");
    // Both selectors, not only the action: the transcription one used to be
    // hidden under 600 px and is now part of the floating layout.
    for (const selector of [".action-control .select-trigger", ".engine-control .select-trigger"]) await inView(selector);
    await page.getByRole("button", { name: "Start recording", exact: true }).click();
    await expect(page.getByRole("button", { name: "Pause recording" })).toBeVisible();
    for (const selector of [".mic-button", ".pause-button", ".copy-button", ".output-bar"]) await inView(selector);
    const action = await page.locator(".controls").boundingBox();
    const mic = await page.locator(".mic-button").boundingBox();
    expect(mic!.y - action!.y - action!.height).toBeGreaterThanOrEqual(14);
    expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBe(true);
    await page.screenshot({ path: testInfo.outputPath("floating-recording-dark.png") });
    await page.getByRole("button", { name: "Pause recording" }).click();
    await inView(".pause-button");
    await page.getByRole("button", { name: "Stop recording", exact: true }).click();
    await expect(page.locator(".copy-button")).toContainText("Copied");
    await expect(disclosure).toHaveAttribute("aria-expanded", "false");
    await page.evaluate(() => document.documentElement.dataset.theme = "light");
    await page.waitForTimeout(350); // Capture the settled light theme, not its transition.
    await page.screenshot({ path: testInfo.outputPath("floating-copied-light.png") });
    await disclosure.click();
    await expect(page.getByRole("region", { name: "Transcript", exact: true })).toBeVisible();
    await page.getByRole("combobox", { name: "Recent texts" }).click();
    await expect(page.getByRole("option", { name: /Ideas for the next release/ })).toBeVisible();
    await page.getByRole("option", { name: /Ideas for the next release/ }).click();
    await page.keyboard.press("Control+Shift+C");
    await expect.poll(() => page.evaluate(() => Reflect.get(window, "__copiedText"))).toContain("A previous thought");
  });
}

test("the dictation key and the typing method are reachable and readable in Settings", async ({ page }, testInfo) => {
  await page.goto("/");
  await page.getByRole("button", { name: "Open settings" }).click();
  await expect(page.getByRole("dialog", { name: "Settings" })).toBeVisible();
  await page.getByRole("tab", { name: /General/ }).click();

  await expect(page.getByRole("tabpanel").getByRole("heading").first()).toHaveText("Dictation shortcut");
  await page.getByRole("tab", { name: "Recording", exact: true }).click();
  await expect(page.getByRole("heading", { name: "Dictation shortcut", exact: true })).toHaveCount(0);
  await page.getByRole("tab", { name: "General", exact: true }).click();

  // The shortcut is recorded, not typed: real key presses through the browser.
  const shortcut = page.getByRole("button", { name: /Shortcut/ });
  await shortcut.scrollIntoViewIfNeeded();
  await expect(shortcut).toContainText("Ctrl+Alt+D");
  await page.screenshot({ path: testInfo.outputPath("dictation-key-dark.png") });

  await shortcut.click();
  await expect(shortcut).toHaveAttribute("aria-pressed", "true");
  await expect(shortcut).toContainText("Press shortcut");
  await expect.poll(() => page.evaluate(() => Reflect.get(window, "__appliedHotkey"))).toBe(null);
  await page.screenshot({ path: testInfo.outputPath("dictation-key-listening.png") });

  // A key with no modifier is refused where it was pressed, and changes nothing.
  await page.keyboard.press("KeyK");
  await expect(page.getByText(/needs a modifier/).first()).toBeVisible();
  await expect(shortcut).toHaveAttribute("aria-pressed", "true");

  await page.keyboard.press("Control+Alt+KeyK");
  await expect(shortcut).toContainText("Ctrl+Alt+K");
  await expect(shortcut).toHaveAttribute("aria-pressed", "false");
  // Released only while it was being read; the working key is back already.
  await expect.poll(() => page.evaluate(() => Reflect.get(window, "__appliedHotkey"))).toBe("Ctrl+Alt+D");

  const shortcutBox = (await shortcut.boundingBox())!;
  const generalScroll = (await page.locator(".settings-scroll").boundingBox())!;
  expect(shortcutBox.x).toBeGreaterThanOrEqual(generalScroll.x - 1);
  expect(shortcutBox.x + shortcutBox.width).toBeLessThanOrEqual(generalScroll.x + generalScroll.width + 1);
  await page.getByRole("tab", { name: "Output", exact: true }).click();

  // Paste is the default; the keystroke delay only appears once it is needed,
  // so the common case stays a single choice.
  await expect(page.getByRole("spinbutton", { name: /Delay between keystrokes/ })).toBeHidden();
  await page.getByRole("button", { name: "Keystrokes", exact: true }).click();
  const delay = page.getByRole("spinbutton", { name: /Delay between keystrokes/ });
  await expect(delay).toHaveValue("15");

  // Both groups must stay inside the scroll viewport at the default size.
  const scroll = (await page.locator(".settings-scroll").boundingBox())!;
  for (const field of [delay]) {
    const box = (await field.boundingBox())!;
    expect(box.x).toBeGreaterThanOrEqual(scroll.x - 1);
    expect(box.x + box.width).toBeLessThanOrEqual(scroll.x + scroll.width + 1);
  }

  await page.getByRole("button", { name: "Save settings" }).click();
  await expect(page.getByRole("dialog", { name: "Settings" })).toBeHidden();
  expect(await page.evaluate(() => Reflect.get(window, "__appliedHotkey"))).toBe("Ctrl+Alt+K");
  expect(await page.evaluate(() => Reflect.get(window, "__savedSettings"))).toMatchObject({
    typing_method: "keystrokes", typing_delay_ms: 15, global_hotkey: "Ctrl+Alt+K",
  });
});

test("Escape leaves the shortcut alone before it leaves Settings", async ({ page }) => {
  await page.goto("/");
  await page.getByRole("button", { name: "Open settings" }).click();
  await page.getByRole("tab", { name: /General/ }).click();
  const shortcut = page.getByRole("button", { name: /Shortcut/ });
  await shortcut.click();
  await page.keyboard.press("Escape");
  await expect(shortcut).toContainText("Ctrl+Alt+D");
  await expect(page.getByRole("dialog", { name: "Settings" })).toBeVisible();
  await page.keyboard.press("Escape");
  await expect(page.getByRole("dialog", { name: "Settings" })).toHaveCount(0);
  expect(await page.evaluate(() => Reflect.get(window, "__savedSettings"))).toBeNull();
});

test("the startup switch reads and writes the system entry, and only on Save", async ({ page }, testInfo) => {
  await page.goto("/");
  await page.getByRole("button", { name: "Open settings" }).click();
  await page.getByRole("tab", { name: /General/ }).click();
  const startup = page.getByRole("checkbox", { name: /when I sign in/ });
  await startup.scrollIntoViewIfNeeded();
  await expect(startup).toBeEnabled();
  await expect(startup).not.toBeChecked();

  // Cancel writes nothing at all.
  await startup.check();
  await page.screenshot({ path: testInfo.outputPath("startup-dark.png") });
  await page.getByRole("button", { name: "Cancel" }).click();
  expect(await page.evaluate(() => Reflect.get(window, "__autostartWrites"))).toEqual([]);

  await page.getByRole("button", { name: "Open settings" }).click();
  await page.getByRole("tab", { name: /General/ }).click();
  await expect(page.getByRole("checkbox", { name: /when I sign in/ })).not.toBeChecked();
  await page.getByRole("checkbox", { name: /when I sign in/ }).check();
  await page.getByRole("button", { name: "Save settings" }).click();
  await expect(page.getByRole("dialog", { name: "Settings" })).toBeHidden();
  expect(await page.evaluate(() => Reflect.get(window, "__autostartWrites"))).toEqual(["enable"]);

  // Reopened, the switch shows the entry that now exists, and saving again
  // without touching it writes nothing more.
  await page.getByRole("button", { name: "Open settings" }).click();
  await page.getByRole("tab", { name: /General/ }).click();
  await expect(page.getByRole("checkbox", { name: /when I sign in/ })).toBeChecked();
  await page.getByRole("button", { name: "Save settings" }).click();
  await expect(page.getByRole("dialog", { name: "Settings" })).toBeHidden();
  expect(await page.evaluate(() => Reflect.get(window, "__autostartWrites"))).toEqual(["enable"]);
});

for (const theme of ["dark", "light"]) {
  test(`particle ribbon fills the compact recording width in ${theme}`, async ({ page }, testInfo) => {
    await page.setViewportSize({ width: 640, height: 560 });
    await page.emulateMedia({ reducedMotion: "reduce" });
    await page.goto("/");
    await page.evaluate((value) => document.documentElement.dataset.theme = value, theme);
    await page.getByRole("button", { name: "Start recording", exact: true }).click();
    await expect(page.locator("main")).toHaveClass(/recording/);
    const measure = () => page.locator<HTMLCanvasElement>(".signal-field").evaluate((canvas) => {
      const { width, height } = canvas;
      const pixels = canvas.getContext("2d")!.getImageData(0, 0, width, height).data;
      let left = width, right = 0, count = 0;
      for (let y = 0; y < height; y++) for (let x = 0; x < width; x++) {
        if (pixels[(y * width + x) * 4 + 3] > 15) { left = Math.min(left, x); right = Math.max(right, x); count++; }
      }
      return { span: (right - left) / width, count, height: canvas.clientHeight };
    });
    const metrics = await measure();
    expect(metrics.span).toBeGreaterThan(.9);
    expect(metrics.count).toBeGreaterThan(3000);
    expect(metrics.height).toBeGreaterThanOrEqual(140);
    await page.screenshot({ path: testInfo.outputPath(`ribbon-640x560-${theme}.png`) });
    await page.setViewportSize({ width: 360, height: 400 });
    await expect.poll(async () => (await measure()).span).toBeGreaterThan(.85);
    const canvas = await page.locator(".signal-field").boundingBox();
    const controls = await page.locator(".recording-controls").boundingBox();
    expect(canvas!.y + canvas!.height).toBeLessThanOrEqual(controls!.y);
    expect(await page.evaluate(() => document.documentElement.scrollHeight)).toBeLessThanOrEqual(400);
  });
}

test("ribbon stops when hidden, resumes, and repaints a paused theme and resize", async ({ page }) => {
  const errors: string[] = [];
  page.on("pageerror", (error) => errors.push(error.message));
  await page.goto("/");
  await page.getByRole("button", { name: "Start recording", exact: true }).click();
  const first = await ribbonPixels(page);
  await expect.poll(() => ribbonPixels(page)).not.toBe(first);
  await page.evaluate(() => {
    Object.defineProperty(document, "hidden", { configurable: true, value: true });
    document.dispatchEvent(new Event("visibilitychange"));
  });
  const hidden = await ribbonPixels(page);
  await page.waitForTimeout(200);
  expect(await ribbonPixels(page)).toBe(hidden);
  await page.evaluate(() => {
    delete (document as Partial<Document>).hidden;
    document.dispatchEvent(new Event("visibilitychange"));
  });
  await expect.poll(() => ribbonPixels(page)).not.toBe(hidden);
  await page.getByRole("button", { name: "Pause recording", exact: true }).click();
  const dark = await ribbonPixels(page);
  await page.evaluate(() => document.documentElement.dataset.theme = "light");
  await expect.poll(() => ribbonPixels(page)).not.toBe(dark);
  await page.setViewportSize({ width: 640, height: 560 });
  await expect.poll(() => page.locator<HTMLCanvasElement>(".signal-field").evaluate((canvas) => canvas.width)).toBe(600);
  const resized = await ribbonPixels(page);
  await page.waitForTimeout(200);
  expect(await ribbonPixels(page)).toBe(resized);
  await page.emulateMedia({ reducedMotion: "reduce" });
  await page.getByRole("button", { name: "Resume recording", exact: true }).click();
  const still = await ribbonPixels(page);
  await page.waitForTimeout(200);
  expect(await ribbonPixels(page)).toBe(still);
  await page.emulateMedia({ reducedMotion: "no-preference" });
  await expect.poll(() => ribbonPixels(page)).not.toBe(still);
  expect(errors).toEqual([]);
});

test("Finish delivers immediately while the ribbon softly settles, then stops", async ({ page }) => {
  await page.goto("/");
  await page.getByRole("button", { name: "Start recording", exact: true }).click();
  await expect(page.locator(".timer")).toContainText("00:01");
  await page.getByRole("button", { name: "Stop recording", exact: true }).click();
  // Output and next-record controls do not wait for the decorative release.
  await expect(page.getByText("Copied to clipboard", { exact: true })).toBeVisible();
  await expect(page.getByRole("button", { name: "Start recording", exact: true })).toBeEnabled();
  const released = await ribbonPixels(page);
  const opacity = () => page.locator(".signal-field").evaluate((canvas) => Number(getComputedStyle(canvas).opacity));
  expect(await opacity()).toBeGreaterThan(.8);
  await page.waitForTimeout(200);
  expect(await ribbonPixels(page)).not.toBe(released);
  expect(await opacity()).toBeGreaterThan(.5);
  await expect.poll(opacity).toBe(.5);
  const rest = await ribbonPixels(page);
  await page.waitForTimeout(200);
  expect(await ribbonPixels(page)).toBe(rest);
});

test("a new recording interrupts the release and hidden completion has no replay", async ({ page }) => {
  await page.goto("/");
  await page.getByRole("button", { name: "Start recording", exact: true }).click();
  await expect(page.locator(".timer")).toContainText("00:01");
  await page.getByRole("button", { name: "Stop recording", exact: true }).click();
  await page.getByRole("button", { name: "Start recording", exact: true }).click();
  await expect(page.locator("main")).toHaveClass(/recording/);
  const restarted = await ribbonPixels(page);
  await expect.poll(() => ribbonPixels(page)).not.toBe(restarted);
  await page.getByRole("button", { name: "Stop recording", exact: true }).click();
  await page.evaluate(() => {
    Object.defineProperty(document, "hidden", { configurable: true, value: true });
    document.dispatchEvent(new Event("visibilitychange"));
  });
  const hidden = await ribbonPixels(page);
  await page.waitForTimeout(150);
  expect(await ribbonPixels(page)).toBe(hidden);
  await page.evaluate(() => {
    delete (document as Partial<Document>).hidden;
    document.dispatchEvent(new Event("visibilitychange"));
  });
  await expect.poll(() => page.locator(".signal-field").evaluate((canvas) => getComputedStyle(canvas).opacity)).toBe("0.5");
  const restored = await ribbonPixels(page);
  await page.waitForTimeout(200);
  expect(await ribbonPixels(page)).toBe(restored);

  // The same boundary from the remote-intent path, which is where the reserved
  // dictation key and `utterform --toggle` arrive. A toggle while the transcript
  // is still being processed is dropped and never replayed; the first toggle
  // after the answer starts exactly one recording, and does so before the
  // ribbon has settled. (The Done cue itself is native and not exercised here.)
  await page.evaluate(() => {
    const ipc = Reflect.get(window, "__TAURI_INTERNALS__");
    const invoke = ipc.invoke;
    Reflect.set(window, "__startCount", 0);
    ipc.invoke = async (command: string, args: Record<string, unknown>) => {
      if (command === "start_recording") Reflect.set(window, "__startCount", Reflect.get(window, "__startCount") + 1);
      if (command === "finish_recording") await new Promise<void>((resolve) => Reflect.set(window, "__completeTranscription", resolve));
      return invoke(command, args);
    };
  });
  const startCount = () => page.evaluate(() => Reflect.get(window, "__startCount") as number);
  const toggle = () => page.evaluate(() => Reflect.get(window, "__emitRemoteIntent")("toggle"));
  const complete = () => page.evaluate(() => Reflect.get(window, "__completeTranscription")());
  await toggle();
  await expect(page.locator("main")).toHaveClass(/recording/);
  await expect.poll(startCount).toBe(1);
  await toggle();
  await expect(page.locator("main")).toHaveClass(/processing/);
  await toggle();
  await toggle();
  await page.waitForTimeout(150);
  expect(await startCount()).toBe(1);
  await complete();
  await expect(page.getByText("Copied to clipboard", { exact: true })).toBeVisible();
  await page.waitForTimeout(250);
  expect(await startCount()).toBe(1);
  // A burst inside one moment — a bouncing key, a double press — is one start,
  // not a start followed by a stop. Sent from one script so nothing can run
  // between them.
  await page.evaluate(() => {
    const emit = Reflect.get(window, "__emitRemoteIntent");
    emit("toggle");
    emit("toggle");
    emit("start");
  });
  await expect(page.locator("main")).toHaveClass(/recording/);
  await page.waitForTimeout(150);
  expect(await startCount()).toBe(2);
  await expect(page.locator("main")).toHaveClass(/recording/);
  await toggle();
  await expect(page.locator("main")).toHaveClass(/processing/);
  await complete();
  await expect(page.getByText("Copied to clipboard", { exact: true })).toBeVisible();
  expect(await startCount()).toBe(2);
});

for (const viewport of [{ width: 920, height: 720 }, { width: 360, height: 400 }]) {
  test(`the transcription selector writes at once and stays reachable at ${viewport.width}×${viewport.height}`, async ({ page }, testInfo) => {
    await page.setViewportSize(viewport);
    await page.goto("/");
    const inView = async (locator: ReturnType<typeof page.locator>) => {
      await expect(locator).toBeVisible();
      const box = (await locator.boundingBox())!;
      expect(box.x).toBeGreaterThanOrEqual(0);
      expect(box.y).toBeGreaterThanOrEqual(0);
      expect(box.x + box.width).toBeLessThanOrEqual(viewport.width);
      expect(box.y + box.height).toBeLessThanOrEqual(viewport.height);
    };
    const action = page.getByRole("combobox", { name: "Action", exact: true });
    const transcription = page.getByRole("combobox", { name: "Transcription", exact: true });
    await inView(action);
    await inView(transcription);
    await expect(transcription).toContainText("GPT Transcribe");
    // A label that is cut off is not reachable in any useful sense.
    for (const control of [action, transcription]) {
      expect(await control.locator("strong").evaluate((el) => el.scrollWidth <= el.clientWidth)).toBe(true);
    }
    const saved = () => page.evaluate(() => Reflect.get(window, "__savedSettings") as { engine: string; cloud_model: string } | null);

    // By mouse: every option in view, the choice written without Settings.
    await transcription.click();
    for (const name of ["GPT Transcribe", "GPT Live Transcribe", "Local Whisper"]) await inView(page.getByRole("option", { name: new RegExp(`^${name} `) }));
    // Opaque once its opening motion has run: nothing behind it may show through.
    await expect.poll(() => page.locator(".select-options").evaluate((el) => getComputedStyle(el).opacity)).toBe("1");
    await page.screenshot({ path: testInfo.outputPath("transcription-menu.png") });
    await page.getByRole("option", { name: /^Local Whisper / }).click();
    await expect.poll(async () => (await saved())?.engine).toBe("local_whisper");
    expect((await saved())?.cloud_model).toBe("gpt_transcribe");
    await expect(transcription).toContainText("Local Whisper");
    await expect(page.getByRole("dialog")).toHaveCount(0);

    // By keyboard: the menu opens on the arrow, Enter writes, focus stays put.
    await transcription.focus();
    await page.keyboard.press("ArrowDown");
    await expect(page.getByRole("listbox", { name: "Transcription" })).toBeVisible();
    await page.keyboard.press("ArrowUp");
    await page.keyboard.press("Enter");
    await expect.poll(async () => (await saved())?.cloud_model).toBe("gpt_live_transcribe");
    expect((await saved())?.engine).toBe("open_ai");
    await expect(transcription).toBeFocused();
    await expect(transcription).toContainText("GPT Live Transcribe");
    await expect(page.getByRole("combobox", { name: "Action", exact: true })).toHaveCount(0);
    await expect(page.getByText(/Place the cursor in your text field/)).toBeVisible();
    expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBe(true);

    // Settings shows the same pair, and Cancel there leaves it alone.
    await page.getByRole("button", { name: "Open settings" }).click();
  await page.getByRole("tab", { name: "AI & Models", exact: true }).click();
    await expect(page.getByRole("combobox", { name: "Transcription mode" })).toContainText("GPT Live Transcribe");
    await page.getByRole("button", { name: "Cancel" }).click();
    await expect(transcription).toContainText("GPT Live Transcribe");
    await page.screenshot({ path: testInfo.outputPath("transcription-live.png") });
  });
}

test("processing remains fluid until delayed transcription completes", async ({ page }) => {
  await page.goto("/");
  await page.evaluate(() => {
    const ipc = Reflect.get(window, "__TAURI_INTERNALS__");
    const invoke = ipc.invoke;
    ipc.invoke = async (command: string, args: Record<string, unknown>) => {
      if (command === "finish_recording") await new Promise<void>((resolve) => Reflect.set(window, "__completeTranscription", resolve));
      return invoke(command, args);
    };
  });
  await page.getByRole("button", { name: "Start recording", exact: true }).click();
  await expect(page.locator(".timer")).toContainText("00:01");
  await page.getByRole("button", { name: "Pause recording", exact: true }).click();
  const paused = await ribbonPixels(page);
  await page.getByRole("button", { name: "Stop recording", exact: true }).click();
  await expect(page.locator("main")).toHaveClass(/processing/);
  await expect.poll(() => ribbonPixels(page)).not.toBe(paused);
  await page.waitForTimeout(750);
  const processing = await ribbonPixels(page);
  await page.waitForTimeout(150);
  expect(await ribbonPixels(page)).not.toBe(processing);
  await page.evaluate(() => Reflect.get(window, "__completeTranscription")());
  await expect(page.getByText("Copied to clipboard", { exact: true })).toBeVisible();
  const released = await ribbonPixels(page);
  await page.waitForTimeout(150);
  expect(await ribbonPixels(page)).not.toBe(released);
  await expect.poll(() => page.locator(".signal-field").evaluate((canvas) => getComputedStyle(canvas).opacity)).toBe("0.5");
  const rest = await ribbonPixels(page);
  await page.waitForTimeout(150);
  expect(await ribbonPixels(page)).toBe(rest);
});


test("live settings and blocked transcript remain usable in the production bundle", async ({ page }, testInfo) => {
  const errors: string[] = [];
  page.on("pageerror", (error) => errors.push(error.message));
  await page.goto("/");
  await page.getByRole("button", { name: "Open settings" }).click();
  await page.getByRole("tab", { name: "AI & Models", exact: true }).click();
  await page.getByRole("combobox", { name: "Transcription mode" }).click();
  await page.getByRole("option", { name: /GPT Live Transcribe/ }).click();
  await page.screenshot({ path: testInfo.outputPath("live-settings.png") });
  await page.getByRole("button", { name: "Save settings" }).click();
  await expect(page.getByRole("combobox", { name: "Action", exact: true })).toHaveCount(0);
  await expect(page.getByText(/Place the cursor in your text field/)).toBeVisible();
  await page.evaluate(() => Reflect.get(window, "__emitRemoteIntent")("start"));
  await expect(page.getByRole("region", { name: "Live transcript", exact: true })).toContainText("Live words at the cursor");
  await expect(page.getByText("Focus changed. Insertion is paused.")).toBeVisible();
  await expect(page.getByRole("button", { name: "Stop recording", exact: true })).toBeVisible();
  await page.screenshot({ path: testInfo.outputPath("live-focus-paused.png") });
  await page.evaluate(() => Reflect.get(window, "__emitRemoteIntent")("cancel"));
  await expect(page.getByRole("region", { name: "Transcript", exact: true })).toContainText("Live words at the cursor");
  await page.getByRole("button", { name: /^Copy/ }).click();
  await expect.poll(() => page.evaluate(() => Reflect.get(window, "__copiedText"))).toBe("Live words at the cursor");
  expect(errors).toEqual([]);
});

for (const viewport of [{ width: 360, height: 400 }, { width: 920, height: 720 }]) {
  test(`settings keep edits across categories and restore Cancel at ${viewport.width}×${viewport.height}`, async ({ page }, testInfo) => {
    await page.setViewportSize(viewport);
    await page.goto("/");
    await page.getByRole("button", { name: "Open settings" }).click();
    const tabs = ["General", "Recording", "AI & Models", "Actions", "Output"];
    const tab = (name: string) => page.getByRole("tab", { name, exact: true });
    await expect(page.getByRole("tab")).toHaveText(tabs);
    await tab("General").focus();
    // Keyboard navigation wraps the rail and keeps every category visible.
    for (const name of [...tabs.slice(1), "General"]) {
      await page.keyboard.press("ArrowRight");
      await expect(tab(name)).toBeFocused();
      await expect(tab(name)).toHaveAttribute("aria-selected", "true");
      await expect(tab(name)).toBeInViewport();
    }
    await page.keyboard.press("ArrowLeft");
    await expect(tab("Output")).toBeFocused();

    const editAcrossTabs = async () => {
      await tab("General").click();
      await page.getByRole("button", { name: "Light", exact: true }).click();
      await tab("Recording").click();
      await page.getByRole("combobox", { name: "Microphone", exact: true }).click();
      await page.getByRole("option", { name: /Studio microphone/ }).click();
      await page.getByRole("textbox", { name: "Vocabulary", exact: true }).fill("Utterform\nCareum");
      await tab("AI & Models").click();
      await expect(page.getByLabel(/API key/)).toBeVisible();
      await page.getByRole("button", { name: "Low", exact: true }).click();
      await tab("Actions").click();
      await page.getByRole("textbox", { name: "Prompt name", exact: true }).fill("Tidy up");
      await tab("Output").click();
      await page.getByRole("button", { name: "Keystrokes", exact: true }).click();
      await page.getByRole("spinbutton", { name: /Delay between keystrokes/ }).fill("30");
      await page.getByRole("checkbox", { name: /Remember the last 100 texts/ }).uncheck();
      expect(await page.evaluate(() => document.documentElement.scrollWidth <= window.innerWidth)).toBe(true);
    };
    await editAcrossTabs();
    await page.getByRole("button", { name: "Cancel", exact: true }).click();
    expect(await page.evaluate(() => Reflect.get(window, "__savedSettings"))).toBeNull();
    await expect(page.locator("html")).toHaveAttribute("data-theme", "dark");

    await page.getByRole("button", { name: "Open settings" }).click();
    await tab("Recording").click();
    await expect(page.getByRole("combobox", { name: "Microphone", exact: true })).toHaveText("System default");
    await expect(page.getByRole("textbox", { name: "Vocabulary", exact: true })).toHaveValue("");
    await tab("AI & Models").click();
    await expect(page.getByRole("button", { name: "Auto", exact: true })).toHaveClass(/active/);
    await tab("Actions").click();
    await expect(page.getByRole("textbox", { name: "Prompt name", exact: true })).toHaveValue("Clean");
    await tab("Output").click();
    await expect(page.getByRole("button", { name: "Paste", exact: true })).toHaveClass(/active/);
    await expect(page.getByRole("checkbox", { name: /Remember the last 100 texts/ })).toBeChecked();
    await editAcrossTabs();
    await page.getByRole("button", { name: "Save settings", exact: true }).click();
    expect(await page.evaluate(() => Reflect.get(window, "__savedSettings"))).toMatchObject({
      theme: "light", input_device: "test-mic", vocabulary: ["Utterform", "Careum"],
      text_effort: "low", action_overrides: { clean: { name: "Tidy up" } },
      typing_method: "keystrokes", typing_delay_ms: 30, history_enabled: false,
    });
    // Capture every reorganised panel in both themes for visual review.
    await page.getByRole("button", { name: "Open settings" }).click();
    for (const theme of ["Light", "Dark"]) {
      await tab("General").click();
      await page.getByRole("button", { name: theme, exact: true }).click();
      for (const name of tabs) {
        await tab(name).click();
        await page.locator(".settings-scroll").evaluate((el) => el.scrollTop = 0);
        await page.screenshot({ animations: "disabled", path: testInfo.outputPath(`${name.replace(/\W+/g, "-")}-${theme}.png`) });
      }
    }
  });
}

for (const viewport of [
  { width: 920, height: 720 },
  { width: 1280, height: 1000 },
  { width: 360, height: 400 },
  { width: 920, height: 400 },
]) {
  test(`settings geometry stays calm at ${viewport.width}×${viewport.height}`, async ({ page }, testInfo) => {
    await page.setViewportSize(viewport);
    await page.emulateMedia({ reducedMotion: "reduce" });
    await page.goto("/");
    await page.getByRole("button", { name: "Open settings" }).click();
    const tabs = ["General", "Recording", "AI & Models", "Actions", "Output"];
    const panel = page.getByRole("tabpanel");
    const geometry = () => page.locator(".settings-modal, .modal-header, .settings-rail, .settings-scroll, .modal-actions").evaluateAll(
      (elements) => elements.map((element) => {
        const { x, y, width, height } = element.getBoundingClientRect();
        return { x, y, width, height };
      }),
    );
    const original = await geometry();
    const cardWidth = (await panel.locator(".setting-group").first().boundingBox())!.width;
    for (const rect of original) {
      expect(rect.x).toBeGreaterThanOrEqual(0);
      expect(rect.y).toBeGreaterThanOrEqual(0);
      expect(rect.x + rect.width).toBeLessThanOrEqual(viewport.width);
      expect(rect.y + rect.height).toBeLessThanOrEqual(viewport.height);
    }
    for (const name of [...tabs, ...tabs.toReversed()]) {
      const tab = page.getByRole("tab", { name, exact: true });
      await tab.click();
      await expect(tab).toBeInViewport({ ratio: 1 });
      await expect.poll(geometry).toEqual(original);
      expect(await panel.evaluate((element) => element.scrollTop)).toBe(0);
      expect((await panel.locator(".setting-group").first().boundingBox())!.width).toBe(cardWidth);
    }
    await page.getByRole("tab", { name: "Recording", exact: true }).click();
    // Scroll the dense panel to its last control; the chrome must not move.
    await page.getByRole("textbox", { name: /Recording context/ }).scrollIntoViewIfNeeded();
    expect(await panel.evaluate((element) => element.scrollTop)).toBeGreaterThan(0);
    expect(await geometry()).toEqual(original);
    await expect(page.getByRole("button", { name: "Save settings", exact: true })).toBeInViewport({ ratio: 1 });
    await page.getByRole("tab", { name: "General", exact: true }).click();
    expect(await panel.evaluate((element) => element.scrollTop)).toBe(0);
    await page.screenshot({ path: testInfo.outputPath("stable-settings.png") });

    // Resizing an already open dialog recomputes its viewport cap, not a cached size.
    await page.setViewportSize({ width: 480, height: 480 });
    const resized = await geometry();
    for (const name of tabs) {
      await page.getByRole("tab", { name, exact: true }).click();
      await expect.poll(geometry).toEqual(resized);
    }
    await expect(page.getByRole("button", { name: "Cancel", exact: true })).toBeInViewport({ ratio: 1 });
  });
}

const LOG_PATH = "/home/tester/.local/share/software.jli.utterform/logs/utterform.log";

for (const viewport of [{ width: 920, height: 720 }, { width: 360, height: 400 }]) {
  test(`support actions run at once from General and stay reachable at ${viewport.width}×${viewport.height}`, async ({ page }, testInfo) => {
    const errors: string[] = [];
    page.on("pageerror", (error) => errors.push(error.message));
    await page.setViewportSize(viewport);
    await page.emulateMedia({ reducedMotion: "reduce" });
    await page.goto("/");
    await page.getByRole("button", { name: "Open settings" }).click();
    await page.getByRole("tab", { name: "General", exact: true }).click();
    const panel = page.getByRole("tabpanel");
    await expect(panel.getByRole("heading").last()).toHaveText("Support & diagnostics");
    const scroll = page.locator(".settings-scroll");
    const reachable = async (control: ReturnType<typeof page.getByRole>) => {
      await control.scrollIntoViewIfNeeded();
      await expect(control).toBeVisible();
      await control.focus();
      await expect(control).toBeFocused();
      const area = (await scroll.boundingBox())!;
      const box = (await control.boundingBox())!;
      expect(box.x).toBeGreaterThanOrEqual(area.x - 1);
      expect(box.y).toBeGreaterThanOrEqual(area.y - 1);
      expect(box.x + box.width).toBeLessThanOrEqual(area.x + area.width + 1);
      expect(box.y + box.height).toBeLessThanOrEqual(area.y + area.height + 1);
    };
    const copy = page.getByRole("button", { name: "Copy diagnostics", exact: true });
    const openFile = page.getByRole("button", { name: "Open log file", exact: true });
    const openFolder = page.getByRole("button", { name: "Open log folder", exact: true });
    for (const control of [copy, openFile, openFolder]) await reachable(control);
    const path = page.getByText(LOG_PATH, { exact: true });
    await path.scrollIntoViewIfNeeded();
    await expect(path).toBeVisible();
    expect(await path.evaluate((element) => element.getBoundingClientRect().right <= element.closest(".setting-group")!.getBoundingClientRect().right + 1)).toBe(true);
    expect(await scroll.evaluate((element) => element.scrollWidth <= element.clientWidth)).toBe(true);
    expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBe(true);

    // From the keyboard, at once, with the result announced in the group.
    await copy.focus();
    await page.keyboard.press("Enter");
    await expect(page.getByRole("status").filter({ hasText: "Diagnostics copied" })).toHaveText("Diagnostics copied · 12.4 KB · 318 lines");
    await openFile.click();
    await expect(page.getByText("Log file opened", { exact: true })).toBeVisible();
    await page.evaluate(() => Reflect.set(window, "__failSupport", "open_log_folder"));
    await openFolder.click();
    await expect(page.getByRole("alert")).toHaveText("The log folder could not be opened (ref 0a1b2c3d-4)");
    await expect(page.getByRole("dialog", { name: "Settings" })).toBeVisible();
    await expect(path).toBeVisible();
    await page.screenshot({ path: testInfo.outputPath("support-dark.png") });
    await page.evaluate(() => document.documentElement.dataset.theme = "light");
    await expect(page.getByRole("alert")).toBeVisible();
    await page.screenshot({ path: testInfo.outputPath("support-light.png") });

    // The sound test stays where recording is configured, without the path.
    await page.getByRole("tab", { name: "Recording", exact: true }).click();
    await expect(page.getByRole("button", { name: "Test sounds (5s delay)" })).toBeVisible();
    await expect(page.getByText(LOG_PATH, { exact: true })).toHaveCount(0);
    await page.getByRole("tab", { name: "General", exact: true }).click();

    // Cancel has nothing to undo, and nothing was saved.
    await page.getByRole("button", { name: "Cancel", exact: true }).click();
    await expect(page.getByRole("dialog")).toHaveCount(0);
    expect(await page.evaluate(() => Reflect.get(window, "__savedSettings"))).toBeNull();
    expect(await page.evaluate(() => Reflect.get(window, "__supportCalls"))).toEqual(["copy_diagnostics", "open_log_file", "open_log_folder"]);
    expect(errors).toEqual([]);
  });
}

test("an error the interface did not catch reaches the local log once, without state", async ({ page }) => {
  await page.goto("/");
  await expect(page.getByRole("button", { name: "Start recording", exact: true })).toBeEnabled();
  await page.evaluate(() => {
    setTimeout(() => { throw new TypeError("synthetic interface failure"); });
    void Promise.reject({ transcript: "CANARY never leaves the page" });
  });
  await expect.poll(() => page.evaluate(() => (Reflect.get(window, "__frontendReports") as unknown[]).length)).toBe(2);
  const reports = await page.evaluate(() => Reflect.get(window, "__frontendReports") as Array<Record<string, unknown>>);
  expect(reports.map((report) => report.source).sort()).toEqual(["unhandled_rejection", "window.error"]);
  expect(reports.find((report) => report.source === "window.error")).toMatchObject({ message: "TypeError: synthetic interface failure", phase: "idle" });
  expect(JSON.stringify(reports)).not.toContain("CANARY");
  expect(JSON.stringify(reports)).not.toContain("127.0.0.1");
});
