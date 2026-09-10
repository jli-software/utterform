// Signal's native SVG tile drives every desktop bundle, installer and tray icon.
// The UI uses the same angular U without the tile so it can follow the theme.
import { execFileSync } from "node:child_process";
import { copyFileSync, mkdtempSync, readdirSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { fileURLToPath } from "node:url";

const root = fileURLToPath(new URL("../", import.meta.url));
const icons = join(root, "src-tauri", "icons");
const output = mkdtempSync(join(tmpdir(), "utterform-icons-"));
try {
  execFileSync(process.execPath, [join(root, "node_modules/@tauri-apps/cli/tauri.js"), "icon", join(icons, "app-icon.svg"), "--output", output], { stdio: "inherit" });
  // Tauri's ICNS encoder emits an unordered collection of image/mask chunks.
  // Canonicalize their order so regenerating unchanged artwork is byte-stable.
  const icnsPath = join(output, "icon.icns");
  const icns = readFileSync(icnsPath);
  if (icns.toString("ascii", 0, 4) !== "icns" || icns.readUInt32BE(4) !== icns.length) throw new Error("Invalid ICNS header");
  const chunks = [];
  for (let offset = 8; offset < icns.length;) {
    if (offset + 8 > icns.length) throw new Error("Truncated ICNS chunk");
    const size = icns.readUInt32BE(offset + 4);
    if (size < 8 || offset + size > icns.length) throw new Error("Invalid ICNS chunk size");
    chunks.push(icns.subarray(offset, offset + size));
    offset += size;
  }
  chunks.sort((a, b) => Buffer.compare(a.subarray(0, 4), b.subarray(0, 4)));
  // Named after the product rather than "icon": macOS caches an app's icon
  // under the icon file's identity, and a bundle at the same path whose
  // icon.icns changed kept showing the artwork of an earlier version in the
  // Dock and the app switcher. A name of its own gets a cache entry of its own.
  writeFileSync(join(icons, "Utterform.icns"), Buffer.concat([icns.subarray(0, 8), ...chunks]));
  for (const file of readdirSync(output)) {
    if (/\.(png|ico)$/.test(file)) copyFileSync(join(output, file), join(icons, file));
  }
} finally {
  rmSync(output, { recursive: true, force: true });
}
