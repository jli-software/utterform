#!/bin/sh

set -eu

repo_root="$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)"
release_dir="${1:-${repo_root}/release}"
release_dir="$(CDPATH= cd -- "$release_dir" && pwd)"
prefix="$(mktemp -d)"

cleanup() {
  rm -rf "$prefix"
}
trap cleanup EXIT HUP INT TERM

mkdir -p "$prefix/share/utterform/usr/lib"
: > "$prefix/share/utterform/AppRun"
: > "$prefix/share/utterform/usr/lib/libzstd.so.1"
icon_theme_dir="$prefix/share/icons/hicolor"
installed_icon="$icon_theme_dir/256x256/apps/utterform.png"
mkdir -p "$(dirname "$installed_icon")" "$prefix/test-tools"
printf 'old app logo\n' > "$installed_icon"

# Observe the prefix-scoped refresh even on CI images without desktop tools.
# The second install also verifies that cache-tool failures are non-fatal.
cat > "$prefix/test-tools/gtk-update-icon-cache" <<'EOF'
#!/bin/sh
printf '%s\n' "$*" >> "$UTTERFORM_ICON_CACHE_LOG"
exit "${UTTERFORM_ICON_CACHE_EXIT:-0}"
EOF
chmod +x "$prefix/test-tools/gtk-update-icon-cache"
export PATH="$prefix/test-tools:$PATH"
export UTTERFORM_ICON_CACHE_LOG="$prefix/icon-cache-calls"

UTTERFORM_VERSION=ci \
UTTERFORM_RELEASE_BASE_URL="file://${release_dir}" \
UTTERFORM_PREFIX="$prefix" \
  "$repo_root/scripts/install-linux.sh"

binary="$prefix/share/utterform/bin/utterform"
launcher="$prefix/bin/utterform"

[ -x "$binary" ] || {
  printf 'Utterform package test: installed binary is missing\n' >&2
  exit 1
}
[ -x "$launcher" ] || {
  printf 'Utterform package test: launcher is missing\n' >&2
  exit 1
}
[ ! -e "$prefix/share/utterform/AppRun" ] || {
  printf 'Utterform package test: legacy AppImage runtime was not replaced\n' >&2
  exit 1
}

if readelf -d "$binary" | grep -Eq '\((RPATH|RUNPATH)\)'; then
  printf 'Utterform package test: binary contains an embedded library search path\n' >&2
  exit 1
fi

if find "$prefix/share/utterform" -type f -name '*.so*' -print -quit | grep -q .; then
  printf 'Utterform package test: package unexpectedly contains shared libraries\n' >&2
  exit 1
fi

ldd_output="$(ldd "$binary")"
if printf '%s\n' "$ldd_output" | grep -q 'not found'; then
  printf 'Utterform package test: unresolved shared libraries\n%s\n' "$ldd_output" >&2
  exit 1
fi
if printf '%s\n' "$ldd_output" | grep -Fq "$prefix/share/utterform"; then
  printf 'Utterform package test: binary resolves bundled libraries\n' >&2
  exit 1
fi

grep -Fq 'unset LD_LIBRARY_PATH' "$launcher"
grep -Fq '/bin/utterform" "$@"' "$launcher"

desktop_file="$prefix/share/applications/software.jli.utterform.desktop"
grep -Fq 'StartupWMClass=Utterform' "$desktop_file" || {
  printf 'Utterform package test: desktop entry does not declare the window class\n' >&2
  exit 1
}

cmp "$installed_icon" "$prefix/share/utterform/share/utterform.png" || {
  printf 'Utterform package test: previous desktop logo was not replaced\n' >&2
  exit 1
}
cmp "$installed_icon" "$repo_root/src-tauri/icons/128x128@2x.png" || {
  printf 'Utterform package test: package does not contain the current 256px logo\n' >&2
  exit 1
}
grep -Fxq 'Icon=utterform' "$desktop_file"

printf 'stale reinstalled logo\n' > "$installed_icon"
UTTERFORM_VERSION=ci \
UTTERFORM_RELEASE_BASE_URL="file://${release_dir}" \
UTTERFORM_PREFIX="$prefix" \
UTTERFORM_ICON_CACHE_EXIT=1 \
  "$repo_root/scripts/install-linux.sh"
cmp "$installed_icon" "$repo_root/src-tauri/icons/128x128@2x.png"
[ "$(grep -Fxc -- "-f -t $icon_theme_dir" "$UTTERFORM_ICON_CACHE_LOG")" = 2 ] || {
  printf 'Utterform package test: install/reinstall did not refresh the local icon cache\n' >&2
  exit 1
}

printf 'Utterform package test: system-linked installation, logo replacement and cache refresh verified\n'
