/// Actions per application: which rule a dictation key press meets, and the
/// short list of applications Settings offers when one is added.

import type { AppAction, KnownApp } from "./types";

/// Enough to find the handful of programs someone dictates into.
export const RECENT_APPS = 8;

const plain = (value: string) => value.trim().toLowerCase();

/// A rule written by hand names the application as a person would
/// ("Thunderbird"); one chosen from the list carries its id. Either matches.
export function sameApp(rule: KnownApp, app: KnownApp) {
  const wanted = [plain(rule.id), plain(rule.name)].filter(Boolean);
  return [plain(app.id), plain(app.name)].some((name) => name && wanted.includes(name));
}

/// The rule for the application in front, unless its action no longer exists
/// (a removed custom action): then the action chosen in the window applies.
export function actionFor(rules: AppAction[], app: KnownApp, exists: (action: string) => boolean): AppAction | null {
  return rules.find((rule) => sameApp(rule.app, app) && exists(rule.action)) ?? null;
}

/// Newest first, each application once.
export function rememberApp(recent: KnownApp[], app: KnownApp): KnownApp[] {
  return [app, ...recent.filter((known) => plain(known.id) !== plain(app.id))].slice(0, RECENT_APPS);
}

/// One rule per application: choosing it again changes its action.
export function setRule(rules: AppAction[], app: KnownApp, action: string): AppAction[] {
  const index = rules.findIndex((rule) => sameApp(rule.app, app));
  if (index < 0) return [...rules, { app, action }];
  return rules.map((rule, at) => (at === index ? { ...rule, action } : rule));
}

export function removeRule(rules: AppAction[], app: KnownApp): AppAction[] {
  return rules.filter((rule) => !sameApp(rule.app, app));
}

/// What "+ App" offers: the recent applications that have no rule yet.
export function addable(recent: KnownApp[], rules: AppAction[]): KnownApp[] {
  return recent.filter((app) => !rules.some((rule) => sameApp(rule.app, app)));
}

/// A name typed by hand, matched against the id or the name later.
export function typedApp(name: string): KnownApp | null {
  const trimmed = name.trim();
  return trimmed ? { id: trimmed, name: trimmed } : null;
}
