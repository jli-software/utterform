import { api } from "./api";
import type { FrontendErrorSource } from "./types";

const MAX_MESSAGE = 500;
const MAX_STACK = 2000;
/// Reports one mounted interface may send; an error in a loop is not news
/// after the first few, and the backend counts the rest on its side too.
const MAX_REPORTS = 20;

/// What may leave the page about a failure: a bounded message and a stack
/// without the origin serving the scripts. Never the object itself — a
/// rejection value can be anything, application state included.
export function describeFailure(reason: unknown): { message: string; stack: string | null } {
  if (reason instanceof Error) {
    return {
      message: `${reason.name}: ${reason.message}`.slice(0, MAX_MESSAGE),
      stack: reason.stack ? reduceStack(reason.stack) : null,
    };
  }
  if (typeof reason === "string") return { message: reason.slice(0, MAX_MESSAGE), stack: null };
  return { message: `Non-error value of type ${reason === null ? "null" : typeof reason}`, stack: null };
}

/// `http://tauri.localhost/assets/index-abc.js:1:20` keeps `assets/index-abc.js:1:20`;
/// a `file:///…` location keeps its leading slash, so the backend can still
/// recognise and redact a home directory in it.
export function reduceStack(stack: string) {
  return stack
    .replace(/[a-z][a-z0-9+.-]*:\/\/([^/\s)]*)\//gi, (_match, host: string) => (host ? "" : "/"))
    .slice(0, MAX_STACK);
}

/// Sends failures the backend cannot see to the local log, and answers with
/// the reference to show beside them. It never throws and never rejects: a
/// log that cannot be written must not become the next unhandled rejection.
export function createFailureReporter(currentPhase: () => string) {
  let sent = 0;
  async function report(source: FrontendErrorSource, reason: unknown): Promise<string | null> {
    if (sent >= MAX_REPORTS) return null;
    sent += 1;
    try {
      const { message, stack } = describeFailure(reason);
      return (await api.reportFrontendError({ source, message, stack, phase: currentPhase() })) ?? null;
    } catch {
      return null;
    }
  }
  return {
    report,
    onError: (event: ErrorEvent) => void report("window.error", event.error ?? event.message),
    onUnhandledRejection: (event: PromiseRejectionEvent) => void report("unhandled_rejection", event.reason),
  };
}

/// `Diagnostics copied · 12.4 KB · 318 lines`.
export function copiedSummary(bytes: number, lines: number, truncated: boolean) {
  const size = bytes < 1024 ? `${bytes} bytes` : `${(bytes / 1024).toFixed(1)} KB`;
  return `Diagnostics copied · ${size} · ${lines} ${lines === 1 ? "line" : "lines"}${truncated ? " · newest lines only" : ""}`;
}
