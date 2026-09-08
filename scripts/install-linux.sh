#!/bin/sh

set -eu

version="${UTTERFORM_VERSION:-v0.4.6}"
release_base="${UTTERFORM_RELEASE_BASE_URL:-https://github.com/jli-software/utterform/releases/download/${version}}"
asset="utterform-linux-x86_64-system.tar.gz"
prefix="${UTTERFORM_PREFIX:-${HOME}/.local}"
install_root="${prefix}/share/utterform"
bin_dir="${prefix}/bin"
applications_dir="${prefix}/share/applications"
icons_dir="${prefix}/share/icons/hicolor/256x256/apps"
launcher="${bin_dir}/utterform"

fail() {
  printf 'Utterform installer: %s\n' "$1" >&2
  exit 1
}

[ "$(uname -s)" = "Linux" ] || fail "Linux is required"

case "$(uname -m)" in
  x86_64|amd64) ;;
  *) fail "this Linux build supports x86_64 only" ;;
esac

command -v curl >/dev/null 2>&1 || fail "curl is required"
command -v sha256sum >/dev/null 2>&1 || fail "sha256sum is required"
command -v tar >/dev/null 2>&1 || fail "tar is required"
command -v ldd >/dev/null 2>&1 || fail "ldd is required"

download_dir="$(mktemp -d)"
cleanup() {
  rm -rf "$download_dir"
}
trap cleanup EXIT HUP INT TERM

printf 'Downloading Utterform %s…\n' "$version"
curl -fsSL "${release_base}/${asset}" -o "${download_dir}/${asset}"
curl -fsSL "${release_base}/SHA256SUMS.txt" -o "${download_dir}/SHA256SUMS.txt"

expected="$(sed -n "s/  ${asset}\$//p" "${download_dir}/SHA256SUMS.txt")"
[ -n "$expected" ] || fail "the release checksum is missing"
actual="$(sha256sum "${download_dir}/${asset}" | cut -d ' ' -f 1)"
[ "$actual" = "$expected" ] || fail "SHA-256 verification failed"

tar -xzf "${download_dir}/${asset}" -C "$download_dir"
package_root="${download_dir}/utterform-linux-x86_64-system"
[ -x "${package_root}/bin/utterform" ] || fail "the application package is invalid"

missing_libraries="$(unset LD_LIBRARY_PATH; ldd "${package_root}/bin/utterform" 2>/dev/null | sed -n 's/^[[:space:]]*\([^[:space:]]*\)[[:space:]]*=>[[:space:]]*not found.*$/\1/p')"
if [ -n "$missing_libraries" ]; then
  printf 'Utterform installer: missing system libraries:\n%s\n' "$missing_libraries" >&2
  printf '%s\n' 'On Omarchy/Arch, install the required runtime packages with:' >&2
  printf '%s\n' '  sudo pacman -S --needed webkit2gtk-4.1 gtk3 alsa-lib libayatana-appindicator' >&2
  exit 1
fi

mkdir -p "$prefix/share" "$bin_dir" "$applications_dir" "$icons_dir"
next_root="${prefix}/share/.utterform-next-$$"
previous_root="${prefix}/share/.utterform-previous-$$"
mv "$package_root" "$next_root"

if [ -e "$install_root" ]; then
  mv "$install_root" "$previous_root"
fi
if ! mv "$next_root" "$install_root"; then
  if [ -e "$previous_root" ]; then
    mv "$previous_root" "$install_root"
  fi
  fail "the application could not be installed"
fi
rm -rf "$previous_root"

launcher_tmp="${launcher}.tmp.$$"
{
  printf '%s\n' '#!/bin/sh'
  printf '%s\n' 'unset LD_LIBRARY_PATH'
  printf '%s\n' 'export GDK_BACKEND="${GDK_BACKEND:-x11}"'
  printf 'exec "%s/bin/utterform" "$@"\n' "$install_root"
} > "$launcher_tmp"
chmod +x "$launcher_tmp"
mv "$launcher_tmp" "$launcher"

icon_source="${install_root}/share/utterform.png"
if [ -f "$icon_source" ]; then
  cp "$icon_source" "${icons_dir}/utterform.png"
fi

desktop_file="${applications_dir}/software.jli.utterform.desktop"
desktop_tmp="${desktop_file}.tmp.$$"
{
  printf '%s\n' '[Desktop Entry]'
  printf '%s\n' 'Type=Application'
  printf '%s\n' 'Name=Utterform'
  printf '%s\n' 'Comment=Turn speech into useful text'
  printf 'Exec=%s\n' "$launcher"
  printf '%s\n' 'Icon=utterform'
  printf '%s\n' 'Terminal=false'
  printf '%s\n' 'Categories=Utility;AudioVideo;'
  printf '%s\n' 'StartupNotify=true'
  # Utterform runs as a single instance; the window class lets desktops match
  # the running window to this entry instead of offering another launch.
  printf '%s\n' 'StartupWMClass=Utterform'
} > "$desktop_tmp"
mv "$desktop_tmp" "$desktop_file"

if command -v update-desktop-database >/dev/null 2>&1; then
  update-desktop-database "$applications_dir" >/dev/null 2>&1 || true
fi

printf '\nUtterform %s was installed successfully.\n' "$version"
printf 'Launch it from your app menu or run: %s\n' "$launcher"
case ":${PATH}:" in
  *":${bin_dir}:"*) ;;
  *) printf 'Add %s to PATH to run it as: utterform\n' "$bin_dir" ;;
esac
