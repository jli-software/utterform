import { execFileSync } from "node:child_process";
import { mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { describe, expect, it } from "vitest";

const script = fileURLToPath(new URL("./check-release.mjs", import.meta.url));
const tag = /UTTERFORM_VERSION:-([^}]+)\}/.exec(readFileSync(new URL("./install-linux.sh", import.meta.url), "utf8"))![1];
const check = (value: string) => execFileSync(process.execPath, [script, value], { encoding: "utf8", stdio: "pipe" });

describe("release metadata guard", () => {
  it("accepts the synchronized release metadata", () => {
    expect(check(tag)).toContain(`Release metadata verified: ${tag}`);
  });
  it("accepts Windows CRLF checkouts without changing the source files", () => {
    const fixture = mkdtempSync(join(tmpdir(), "utterform-release-crlf-"));
    try {
      for (const path of ["scripts/check-release.mjs", "package.json", "package-lock.json", "src-tauri/tauri.conf.json", "src-tauri/Cargo.toml", "src-tauri/Cargo.lock", "scripts/install-linux.sh", "CHANGELOG.md", "README.md", `docs/releases/${tag}.md`]) {
        const destination = join(fixture, path);
        mkdirSync(dirname(destination), { recursive: true });
        writeFileSync(destination, readFileSync(new URL(`../${path}`, import.meta.url), "utf8").replace(/\r?\n/g, "\r\n"));
      }
      const lock = join(fixture, "src-tauri/Cargo.lock");
      const before = readFileSync(lock);
      const result = execFileSync(process.execPath, [join(fixture, "scripts/check-release.mjs"), tag], { encoding: "utf8", stdio: "pipe" });
      expect(result).toContain(`Release metadata verified: ${tag}`);
      expect(readFileSync(lock)).toEqual(before);
    } finally {
      rmSync(fixture, { recursive: true, force: true });
    }
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
