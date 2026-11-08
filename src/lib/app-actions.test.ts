import { describe, expect, it } from "vitest";
import { actionFor, addable, rememberApp, removeRule, RECENT_APPS, sameApp, setRule, typedApp } from "./app-actions";

const thunderbird = { id: "org.mozilla.Thunderbird", name: "Thunderbird" };
const ghostty = { id: "com.mitchellh.ghostty", name: "Ghostty" };
const exists = (action: string) => ["plain", "email", "custom:a"].includes(action);

describe("actions per application", () => {
  it("matches a chosen application by id and a typed one by name", () => {
    expect(sameApp(thunderbird, { id: "org.mozilla.thunderbird", name: "Thunderbird" })).toBe(true);
    expect(sameApp(typedApp("thunderbird")!, thunderbird)).toBe(true);
    expect(sameApp(typedApp("Thunder")!, thunderbird)).toBe(false);
    expect(sameApp({ id: "", name: "" }, { id: "", name: "" })).toBe(false);
  });

  it("finds the rule for the application in front", () => {
    const rules = [{ app: thunderbird, action: "email" }, { app: ghostty, action: "plain" }];
    expect(actionFor(rules, ghostty, exists)?.action).toBe("plain");
    expect(actionFor(rules, { id: "chromium", name: "Chromium" }, exists)).toBeNull();
  });

  it("ignores a rule whose action was removed", () => {
    expect(actionFor([{ app: thunderbird, action: "custom:gone" }], thunderbird, exists)).toBeNull();
  });

  it("keeps recent applications newest first, once each, and few", () => {
    let recent = rememberApp([], thunderbird);
    recent = rememberApp(recent, ghostty);
    recent = rememberApp(recent, { ...thunderbird });
    expect(recent.map((app) => app.name)).toEqual(["Thunderbird", "Ghostty"]);
    for (let index = 0; index < 20; index++) recent = rememberApp(recent, { id: `app${index}`, name: `App ${index}` });
    expect(recent).toHaveLength(RECENT_APPS);
  });

  it("keeps one rule per application", () => {
    let rules = setRule([], thunderbird, "plain");
    rules = setRule(rules, typedApp("Thunderbird")!, "email");
    expect(rules).toEqual([{ app: thunderbird, action: "email" }]);
    expect(removeRule(rules, thunderbird)).toEqual([]);
  });

  it("offers only applications without a rule", () => {
    expect(addable([thunderbird, ghostty], [{ app: thunderbird, action: "email" }])).toEqual([ghostty]);
    expect(typedApp("  ")).toBeNull();
  });
});
