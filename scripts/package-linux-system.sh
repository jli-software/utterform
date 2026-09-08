#!/bin/sh

set -eu

repo_root="$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)"
binary="${UTTERFORM_BINARY:-${repo_root}/src-tauri/target/release/utterform}"
output_dir="${UTTERFORM_OUTPUT_DIR:-${repo_root}/release}"
package_name="utterform-linux-x86_64-system"
staging_dir="$(mktemp -d)"

cleanup() {
  rm -rf "$staging_dir"
}
trap cleanup EXIT HUP INT TERM

[ -x "$binary" ] || {
  printf 'Utterform packager: executable not found: %s\n' "$binary" >&2
  exit 1
}

case "$(uname -m)" in
  x86_64|amd64) ;;
  *)
    printf 'Utterform packager: x86_64 build host required\n' >&2
    exit 1
    ;;
esac

if readelf -d "$binary" 2>/dev/null | grep -Eq '\((RPATH|RUNPATH)\)'; then
  printf 'Utterform packager: refusing binary with an embedded library search path\n' >&2
  exit 1
fi

mkdir -p "$staging_dir/$package_name/bin" "$staging_dir/$package_name/share"
install -m 755 "$binary" "$staging_dir/$package_name/bin/utterform"
install -m 644 "$repo_root/src-tauri/icons/128x128@2x.png" "$staging_dir/$package_name/share/utterform.png"

mkdir -p "$output_dir"
tar -C "$staging_dir" -czf "$output_dir/$package_name.tar.gz" "$package_name"

printf '%s\n' "$output_dir/$package_name.tar.gz"
