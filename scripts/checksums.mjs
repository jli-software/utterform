#!/usr/bin/env node
import { createHash } from "node:crypto";
import { readdir, readFile, writeFile } from "node:fs/promises";
import { join } from "node:path";

const directory = process.argv[2] ?? "release";
const files = (await readdir(directory, { withFileTypes: true }))
  .filter((entry) => entry.isFile() && entry.name !== "SHA256SUMS.txt")
  .map((entry) => entry.name)
  .sort();
if (!files.length) throw new Error("No release assets to checksum");
const lines = await Promise.all(files.map(async (name) => {
  const hash = createHash("sha256").update(await readFile(join(directory, name))).digest("hex");
  return `${hash}  ${name}\n`;
}));
await writeFile(join(directory, "SHA256SUMS.txt"), lines.join(""));
console.log(`Checksummed ${files.length} assets in ${directory}`);
