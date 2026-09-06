#!/usr/bin/env bash
# Package only a completely signed Apple-Silicon bundle; ad-hoc is not notarization.
set -euo pipefail

app="${1:-src-tauri/target/release/bundle/macos/Utterform.app}"
output="${2:-release}"
test "$(uname -s)" = Darwin || { echo "Run on macOS" >&2; exit 1; }
test -d "$app" || { echo "Missing app bundle: $app" >&2; exit 1; }

verify_app() {
  local bundle="$1" executable
  executable=$(/usr/libexec/PlistBuddy -c 'Print :CFBundleExecutable' "$bundle/Contents/Info.plist")
  test "$(lipo -archs "$bundle/Contents/MacOS/$executable")" = arm64 || {
    echo "Expected an Apple-Silicon-only app" >&2; return 1;
  }
  # A linker-signed executable alone is insufficient: the assembled bundle's
  # Info.plist and resources must be sealed by the Tauri signing step too.
  test -s "$bundle/Contents/_CodeSignature/CodeResources" || {
    echo "App bundle resources are not sealed; sign the complete app before packaging" >&2
    return 1
  }
  codesign --verify --deep --strict --verbose=2 "$bundle"
}

verify_app "$app"
mkdir -p "$output"
output=$(cd "$output" && pwd)
temporary=$(mktemp -d "${TMPDIR:-/tmp}/utterform-macos.XXXXXX")
mounted=false
cleanup() {
  if "$mounted"; then hdiutil detach "$temporary/mount" -quiet || true; fi
  rm -rf "$temporary"
}
trap cleanup EXIT
mkdir -p "$temporary/staging" "$temporary/mount"
ditto "$app" "$temporary/staging/Utterform.app"
ln -s /Applications "$temporary/staging/Applications"
verify_app "$temporary/staging/Utterform.app"

# Build the image only after signing is complete, without rebundling the app.
dmg="$output/utterform-macos-aarch64.dmg"
hdiutil create -volname Utterform -srcfolder "$temporary/staging" -ov -format UDZO "$dmg"
hdiutil verify "$dmg"
hdiutil attach "$dmg" -nobrowse -readonly -mountpoint "$temporary/mount" -quiet
mounted=true
verify_app "$temporary/mount/Utterform.app"
test "$(readlink "$temporary/mount/Applications")" = /Applications
hdiutil detach "$temporary/mount" -quiet
mounted=false

archive="$output/utterform-macos-aarch64.app.zip"
ditto -c -k --sequesterRsrc --keepParent "$temporary/staging/Utterform.app" "$archive"
mkdir "$temporary/unpacked"
ditto -x -k "$archive" "$temporary/unpacked"
verify_app "$temporary/unpacked/Utterform.app"
echo "Verified ARM app, DMG and ZIP signatures (not Apple-notarized)."
