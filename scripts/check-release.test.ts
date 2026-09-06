import { execFileSync } from "node:child_process";
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { describe, expect, it } from "vitest";

const script = fileURLToPath(new URL("./check-release.mjs", import.meta.url));
const tag = /UTTERFORM_VERSION:-([^}]+)\}/.exec(readFileSync(new URL("./install-linux.sh", import.meta.url), "utf8"))![1];
const check = (value: string) => execFileSync(process.execPath, [script, value], { encoding: "utf8", stdio: "pipe" });

describe("release metadata guard", () => {
  it("accepts the synchronized release metadata", () => {
    expect(check(tag)).toContain(`Release metadata verified: ${tag}`);
  });
  it("rejects malformed tags before resolving release-note paths", () => {
    for (const invalid of ["../v0.3.0", "v01.2.3", "v1.2.3-unknown.1", "v1.2"]) {
      expect(() => check(invalid)).toThrow();
    }
  });
  it("rejects a valid tag that does not match the packaged version", () => {
    expect(() => check("v999999.0.0")).toThrow();
  });
});
