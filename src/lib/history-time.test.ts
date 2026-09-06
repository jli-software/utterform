import { describe, expect, it } from "vitest";
import { formatHistoryTime, fullHistoryDate, historyTimestamp } from "./history-time";
import type { HistoryEntry } from "./types";

const now = new Date(2026, 8, 6, 14, 35).getTime();
const entry: HistoryEntry = { id: `${BigInt(now) * 1_000_000n + 999_999n}`, title: "Idea", text: "Idea", durationMs: 1000, engine: "open_ai" };

describe("European history time", () => {
  it("recovers exact milliseconds from legacy nanosecond IDs", () => {
    expect(historyTimestamp(entry)).toBe(now);
    expect(historyTimestamp({ ...entry, createdAtMs: now - 1000 })).toBe(now - 1000);
    expect(historyTimestamp({ ...entry, id: "unknown" })).toBeNull();
    expect(historyTimestamp({ ...entry, createdAtMs: NaN })).toBeNull();
    expect(historyTimestamp({ ...entry, createdAtMs: 1e20 })).toBeNull();
  });
  it("shows today, 24-hour time, and elapsed whole minutes", () => {
    expect(formatHistoryTime(now - 5 * 60_000, now)).toBe("Today · 14:30 · 5 min ago");
    expect(formatHistoryTime(now - 59_000, now)).toBe("Today · 14:34 · just now");
    expect(formatHistoryTime(now - 2 * 3600_000, now)).toBe("Today · 12:35 · 120 min ago");
  });
  it("switches to DD.MM.YYYY at local midnight and keeps full time in details", () => {
    const previous = new Date(2026, 8, 5, 23, 59).getTime();
    const midnight = new Date(2026, 8, 6, 0, 1).getTime();
    expect(formatHistoryTime(previous, midnight)).toBe("05.09.2026");
    expect(fullHistoryDate(previous)).toBe("05.09.2026 · 23:59");
    expect(formatHistoryTime(new Date(2025, 11, 31).getTime(), now)).toBe("31.12.2025");
  });
  it("does not invent a date or a negative age", () => {
    expect(formatHistoryTime(null, now)).toBe("Date unavailable");
    expect(fullHistoryDate(null)).toBe("Date unavailable");
    expect(formatHistoryTime(now + 60_000, now)).toBe("Today · 14:36");
  });
});
