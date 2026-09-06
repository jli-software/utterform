// Fail before building/publishing if the immutable release tag and metadata drift.
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { resolve } from "node:path";

const root = fileURLToPath(new URL("../", import.meta.url));
const read = (path) => readFileSync(resolve(root, path), "utf8");
const json = (path) => JSON.parse(read(path));
const pkg = json("package.json");
const tag = process.argv[2] ?? `v${pkg.version}`;
const match = /^v((?:0|[1-9]\d*)\.(?:0|[1-9]\d*)\.(?:0|[1-9]\d*))(?:-(alpha|beta|rc)\.([1-9]\d*))?$/.exec(tag);
assert.ok(match, `Unsupported release tag: ${tag}`);
const version = match[1];
const lock = json("package-lock.json");
const versions = {
  "package.json": pkg.version,
  "package-lock.json": lock.version,
  "package-lock.json root": lock.packages[""].version,
  "tauri.conf.json": json("src-tauri/tauri.conf.json").version,
  "Cargo.toml": /\[package\][\s\S]*?\nversion = "([^"]+)"/.exec(read("src-tauri/Cargo.toml"))?.[1],
  "Cargo.lock utterform": /\[\[package\]\]\nname = "utterform"\nversion = "([^"]+)"/.exec(read("src-tauri/Cargo.lock"))?.[1],
};
for (const [path, actual] of Object.entries(versions)) assert.equal(actual, version, `${path} must match ${tag}`);
const installerTag = /UTTERFORM_VERSION:-([^}]+)\}/.exec(read("scripts/install-linux.sh"))?.[1];
assert.equal(installerTag, tag, "Linux installer must download this release by default");
const notes = read(`docs/releases/${tag}.md`);
assert.ok(notes.startsWith(`# Utterform ${version}`), "Release notes need a matching title");
assert.ok(read("CHANGELOG.md").includes(tag), "Changelog must document this tag");
assert.ok(read("README.md").includes(`/releases/tag/${tag}`), "README must link the new release");
console.log(`Release metadata verified: ${tag} (${match[2] ? "prerelease" : "normal release"})`);
