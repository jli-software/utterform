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

printf 'Utterform package test: system-linked installation verified\n'
