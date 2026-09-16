import { beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("./api", () => ({ api: { reportFrontendError: vi.fn() } }));
import { api } from "./api";
import { copiedSummary, createFailureReporter, describeFailure, reduceStack } from "./diagnostics";

beforeEach(() => {
  vi.mocked(api.reportFrontendError).mockReset().mockResolvedValue("0a1b2c3d-1");
});

describe("what a frontend failure report may carry", () => {
  it("keeps an error's name, message and a stack without its origin", () => {
    const error = new TypeError("x is undefined");
    error.stack = "TypeError: x is undefined\n    at start (http://tauri.localhost/assets/index-abc.js:1:2345)\nrecord@tauri://localhost/assets/index-abc.js:9:1";
    expect(describeFailure(error)).toEqual({
      message: "TypeError: x is undefined",
      stack: "TypeError: x is undefined\n    at start (assets/index-abc.js:1:2345)\nrecord@assets/index-abc.js:9:1",
    });
  });

  it("never serializes a value that is not an error", () => {
    expect(describeFailure({ transcript: "CANARY" })).toEqual({ message: "Non-error value of type object", stack: null });
    expect(describeFailure(null).message).toBe("Non-error value of type null");
    expect(describeFailure("plain reason")).toEqual({ message: "plain reason", stack: null });
  });

  it("bounds message and stack", () => {
    const error = new Error("x".repeat(5000));
    error.stack = "y".repeat(10_000);
    const described = describeFailure(error);
    expect(described.message.length).toBeLessThanOrEqual(500);
    expect(described.stack!.length).toBeLessThanOrEqual(2000);
    expect(reduceStack("at a (https://example.test:8443/src/App.svelte:3:4)")).toBe("at a (src/App.svelte:3:4)");
    expect(reduceStack("at b (file:///home/jonas/app/index.js:1:2)")).toBe("at b (/home/jonas/app/index.js:1:2)");
  });
});

describe("the failure reporter", () => {
  it("sends the current phase and answers with the reference", async () => {
    let phase = "recording";
    const reporter = createFailureReporter(() => phase);
    expect(await reporter.report("state", new Error("boom"))).toBe("0a1b2c3d-1");
    phase = "done";
    await reporter.report("autostart", "denied");
    expect(vi.mocked(api.reportFrontendError).mock.calls.map(([report]) => report.phase)).toEqual(["recording", "done"]);
  });

  it("swallows every failure to report, and stops after its allowance", async () => {
    vi.mocked(api.reportFrontendError).mockRejectedValue("no backend");
    const reporter = createFailureReporter(() => "idle");
    for (let index = 0; index < 30; index++) {
      await expect(reporter.report("window.error", new Error(`loop ${index}`))).resolves.toBeNull();
    }
    expect(api.reportFrontendError).toHaveBeenCalledTimes(20);
  });

  it("summarizes a copied report", () => {
    expect(copiedSummary(12_698, 318, false)).toBe("Diagnostics copied · 12.4 KB · 318 lines");
    expect(copiedSummary(512, 1, true)).toBe("Diagnostics copied · 512 bytes · 1 line · newest lines only");
  });
});
