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

# whisper.cpp must be built for the desktops this package is installed on, not
# for the machine that compiled it. ggml does the latter by default, and the
# 0.7.2 release therefore carried AVX-512 and AMX code that killed Utterform
# with an illegal instruction on an Intel N300 as soon as a model was loaded.
# Only ggml's own symbols are examined: crates such as `aes` and `crc32fast`
# carry AVX-512 routines too, but reach them through a CPUID check first,
# which is exactly what the code below them does not do.
if command -v objdump >/dev/null 2>&1; then
  disassembly="$prefix/utterform.disasm"
  objdump -d --no-show-raw-insn "$binary" > "$disassembly"
  wide_in_whisper="$(awk '
    /^[0-9a-f]+ <.*>:$/ { fn = $2 }
    /%zmm|vmovdqu64|vpdpbusd|tileloadd|tdpbssd|ldtilecfg|tilestored/ {
      if (tolower(fn) ~ /ggml|whisper/) count++
    }
    END { print count + 0 }
  ' "$disassembly")"
  if [ "$wide_in_whisper" -ne 0 ]; then
    printf 'Utterform package test: whisper.cpp was built for this machine, not for every supported desktop (%s AVX-512/AMX instructions). See packaging/cmake/portable-cpu.cmake; a cached whisper-rs-sys build may have outlived it.\n' "$wide_in_whisper" >&2
    exit 1
  fi
  # The baseline is still vectorized; a build that lost AVX2 would be portable
  # and far too slow to use.
  if ! grep -q '%ymm' "$disassembly"; then
    printf 'Utterform package test: the build carries no AVX2 code at all\n' >&2
    exit 1
  fi
  rm -f "$disassembly"
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
