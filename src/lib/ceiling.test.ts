import { afterEach, describe, expect, it, vi } from "vitest";
import { withCeiling } from "./ceiling";

afterEach(() => vi.useRealTimers());

describe("processing ceiling", () => {
  it("passes a result through untouched", async () => {
    await expect(withCeiling(Promise.resolve("done"), "too slow", 50)).resolves.toBe("done");
  });

  it("passes a rejection through as its own error", async () => {
    await expect(withCeiling(Promise.reject(new Error("no key")), "too slow", 50)).rejects.toThrow("no key");
  });

  it("gives up with a readable message when nothing ever answers", async () => {
    vi.useFakeTimers();
    const pending = withCeiling(new Promise(() => {}), "Processing did not finish", 1000);
    const settled = expect(pending).rejects.toThrow("Processing did not finish");
    await vi.advanceTimersByTimeAsync(1000);
    await settled;
  });

  it("does not fire once the work has answered", async () => {
    vi.useFakeTimers();
    await expect(withCeiling(Promise.resolve("done"), "too slow", 1000)).resolves.toBe("done");
    // A timer left running would reject after the promise already settled.
    expect(vi.getTimerCount()).toBe(0);
  });
});
