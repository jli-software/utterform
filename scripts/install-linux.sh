#!/bin/sh

set -eu

version="${UTTERFORM_VERSION:-v0.1.0-alpha.1}"
release_base="${UTTERFORM_RELEASE_BASE_URL:-https://github.com/jli-software/utterform/releases/download/${version}}"
asset="utterform-linux-x86_64.AppImage"
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
  *) fail "this test build supports x86_64 only" ;;
esac

command -v curl >/dev/null 2>&1 || fail "curl is required"
command -v sha256sum >/dev/null 2>&1 || fail "sha256sum is required"

download_dir="$(mktemp -d)"
extract_dir="$(mktemp -d)"
cleanup() {
  rm -rf "$download_dir" "$extract_dir"
}
trap cleanup EXIT HUP INT TERM

printf 'Downloading Utterform %s…\n' "$version"
curl -fsSL "${release_base}/${asset}" -o "${download_dir}/${asset}"
curl -fsSL "${release_base}/SHA256SUMS.txt" -o "${download_dir}/SHA256SUMS.txt"

expected="$(sed -n "s/  ${asset}\$//p" "${download_dir}/SHA256SUMS.txt")"
[ -n "$expected" ] || fail "the release checksum is missing"
actual="$(sha256sum "${download_dir}/${asset}" | cut -d ' ' -f 1)"
[ "$actual" = "$expected" ] || fail "SHA-256 verification failed"

chmod +x "${download_dir}/${asset}"
(
  cd "$extract_dir"
  "${download_dir}/${asset}" --appimage-extract >/dev/null
)
[ -x "${extract_dir}/squashfs-root/AppRun" ] || fail "the AppImage could not be extracted"

mkdir -p "$prefix/share" "$bin_dir" "$applications_dir" "$icons_dir"
next_root="${prefix}/share/.utterform-next-$$"
previous_root="${prefix}/share/.utterform-previous-$$"
mv "${extract_dir}/squashfs-root" "$next_root"

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
printf '%s\n' '#!/bin/sh' "exec \"${install_root}/AppRun\" \"\$@\"" > "$launcher_tmp"
chmod +x "$launcher_tmp"
mv "$launcher_tmp" "$launcher"

icon_source="${install_root}/Utterform.png"
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
