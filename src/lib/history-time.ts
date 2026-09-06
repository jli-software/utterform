import type { HistoryEntry } from "./types";

export function historyTimestamp(entry: HistoryEntry): number | null {
  const timestamp = entry.createdAtMs ?? (/^\d{18,20}$/.test(entry.id) ? Number(BigInt(entry.id) / 1_000_000n) : NaN);
  return Number.isFinite(timestamp) && timestamp > 0 && !Number.isNaN(new Date(timestamp).getTime()) ? timestamp : null;
}

const dateFormat = new Intl.DateTimeFormat("de-CH", { day: "2-digit", month: "2-digit", year: "numeric" });
const timeFormat = new Intl.DateTimeFormat("de-CH", { hour: "2-digit", minute: "2-digit", hourCycle: "h23" });

export function fullHistoryDate(timestamp: number | null): string {
  if (timestamp === null) return "Date unavailable";
  return `${dateFormat.format(timestamp)} · ${timeFormat.format(timestamp)}`;
}

export function formatHistoryTime(timestamp: number | null, now: number): string {
  if (timestamp === null) return "Date unavailable";
  const date = new Date(timestamp);
  const today = new Date(now);
  const sameDay = date.getFullYear() === today.getFullYear() && date.getMonth() === today.getMonth() && date.getDate() === today.getDate();
  if (!sameDay) return dateFormat.format(date);
  const minutes = Math.max(0, Math.floor((now - timestamp) / 60_000));
  const relative = timestamp > now ? "" : minutes === 0 ? " · just now" : ` · ${minutes} min ago`;
  return `Today · ${timeFormat.format(date)}${relative}`;
}
