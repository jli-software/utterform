#!/bin/sh

# Utterform as Omarchy's dictation backend, without Omarchy and without the app:
# every Omarchy command and `utterform` itself are stand-ins that record how
# they were called. Covers the installer's commands and menu row, the adapter
# that push-to-talk drives, choosing the backend and removing it again.

set -eu

repo_root="$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)"
work="$(mktemp -d)"
cleanup() {
  rm -rf "$work"
}
trap cleanup EXIT HUP INT TERM

fail() {
  printf 'Omarchy dictation test: %s\n' "$1" >&2
  exit 1
}

package="utterform-linux-x86_64-system"
release="$work/release"
tools="$work/tools"
calls="$work/calls"
mkdir -p "$release" "$tools" "$work/package/$package/bin" "$work/package/$package/share/omarchy"

# --- A release that contains only what the installer needs. ---
printf '#!/bin/sh\nexit 0\n' > "$work/package/$package/bin/utterform"
chmod 755 "$work/package/$package/bin/utterform"
for script in "$repo_root"/packaging/omarchy/omarchy-*; do
  install -m 755 "$script" "$work/package/$package/share/omarchy/"
done
tar -C "$work/package" -czf "$release/$package.tar.gz" "$package"
(cd "$release" && sha256sum "$package.tar.gz" > SHA256SUMS.txt)

# --- Omarchy stand-ins. ---
stub() {
  printf '#!/bin/sh\nprintf "%%s\\n" "%s $*" >> "%s"\n%s\n' "$1" "$calls" "${2:-exit 0}" > "$tools/$1"
  chmod 755 "$tools/$1"
}
stub omarchy-dictation-use
stub omarchy-restart-hyprctl
stub omarchy-notification-send
stub omarchy-i18n 'printf "Hold Right Alt\\n"'
stub uwsm-app
stub setsid
# Whether Utterform is running is the one thing the adapter asks the system.
printf '#!/bin/sh\n[ -e "$UTTERFORM_TEST_RUNNING" ]\n' > "$tools/pgrep"
chmod 755 "$tools/pgrep"

# Only the stand-ins and the base system: a real Omarchy (its commands live in
# /usr/bin too), uwsm or a running Utterform on this machine must not answer.
node_dir="$(dirname "$(command -v node)")"
system="$work/system"
mkdir -p "$system"
for dir in /usr/bin /bin; do
  for command in "$dir"/*; do
    name="${command##*/}"
    case "$name" in
      omarchy-*|uwsm*|utterform|pgrep|setsid) continue ;;
    esac
    [ -e "$system/$name" ] || [ -L "$system/$name" ] || ln -s "$command" "$system/$name"
  done
done
export PATH="$tools:$system:$node_dir"
export HOME="$work/home"
export XDG_CONFIG_HOME="$work/config"
export UTTERFORM_TEST_RUNNING="$work/running"
prefix="$work/prefix"
menu="$XDG_CONFIG_HOME/omarchy/extensions/omarchy-menu.jsonc"
row_id='"setup.default.dictation.utterform"'

install_utterform() {
  UTTERFORM_VERSION=test \
  UTTERFORM_RELEASE_BASE_URL="file://$release" \
  UTTERFORM_PREFIX="$prefix" \
    "$repo_root/scripts/install-linux.sh" > "$work/install.out"
}

# The same reading as Omarchy's shell (shell/plugins/menu/MenuModel.js).
menu_rows() {
  node -e '
    const raw = require("fs").readFileSync(process.argv[1], "utf8");
    const stripped = raw.replace(/^\s*\/\/[^\n]*(\n|$)/gm, "").replace(/,(\s*[}\]])/g, "$1");
    const items = JSON.parse(stripped);
    for (const [id, item] of Object.entries(items)) console.log(id + " " + (item.action || ""));
  ' "$1"
}

# --- Without Omarchy's dictation layer nothing Omarchy-specific is installed. ---
mv "$tools/omarchy-dictation-use" "$work/omarchy-dictation-use"
install_utterform
[ ! -e "$prefix/bin/omarchy-dictation-utterform" ] || fail "commands were installed without Omarchy"
[ ! -e "$menu" ] || fail "a menu was written without Omarchy"
mv "$work/omarchy-dictation-use" "$tools/omarchy-dictation-use"

# --- First install on Omarchy: commands and a new menu file. ---
install_utterform
for name in omarchy-dictation-utterform omarchy-install-dictation-utterform omarchy-remove-dictation-utterform; do
  [ -x "$prefix/bin/$name" ] || fail "$name is missing"
done
[ -f "$menu" ] || fail "the menu extension was not created"
menu_rows "$menu" | grep -qx 'setup.default.dictation.utterform omarchy-default-dictation utterform' \
  || fail "the new menu file does not offer Utterform"
grep -q 'Setup > Defaults > Dictation' "$work/install.out" || fail "the installer does not say where to choose Utterform"

# --- A user's own menu: one line added, a backup kept, nothing else changed. ---
cat > "$menu" <<'EOF'
// My menu
{
  // Personal entries
  "personal": {"icon":"","label":"Personal"},
  "personal.notes": {"icon":"","label":"Notes","action":"omarchy-launch-editor ~/notes"}
  // No trailing comma above, comments below.
  // "about": {"label":"About"},
}
EOF
cp "$menu" "$work/menu.before"
install_utterform
[ "$(grep -cF "$row_id" "$menu")" = 1 ] || fail "the menu row was not added exactly once"
grep -vF "$row_id" "$menu" | cmp -s - "$work/menu.before" || fail "other menu lines changed"
ls "$menu".bak.* >/dev/null 2>&1 || fail "the user's menu was not backed up"
menu_rows "$menu" > "$work/rows"
grep -qx 'personal ' "$work/rows" || fail "the user's own rows were lost"
grep -qx 'setup.default.dictation.utterform omarchy-default-dictation utterform' "$work/rows" \
  || fail "the extended menu does not parse"

# A reinstall leaves a menu with the row alone.
cp "$menu" "$work/menu.installed"
install_utterform
cmp -s "$menu" "$work/menu.installed" || fail "a reinstall changed the menu again"

# A layout the installer does not understand is not edited; the user gets the line.
printf '{ "personal": {"label":"Personal"} }\n' > "$menu"
install_utterform 2> "$work/install.err"
[ "$(cat "$menu")" = '{ "personal": {"label":"Personal"} }' ] || fail "an unexpected menu layout was edited"
grep -qF "$row_id" "$work/install.err" || fail "the user was not given the menu line"
cp "$work/menu.installed" "$menu"

# --- The adapter Omarchy calls. ---
adapter="$prefix/bin/omarchy-dictation-utterform"
cat > "$tools/utterform" <<EOF
#!/bin/sh
printf '%s\n' "utterform \$*" >> "$calls"
EOF
chmod 755 "$tools/utterform"
run_adapter() {
  : > "$calls"
  "$adapter" "$@"
}

# Utterform running: every request is handed straight to it.
touch "$UTTERFORM_TEST_RUNNING"
for request in start stop toggle; do
  run_adapter "$request"
  [ "$(cat "$calls")" = "utterform --$request" ] || fail "$request was not handed to the running app: $(cat "$calls")"
done

# Utterform not running: stop does nothing, start and toggle launch it.
rm "$UTTERFORM_TEST_RUNNING"
run_adapter stop
[ ! -s "$calls" ] || fail "stop started or called Utterform: $(cat "$calls")"
for request in start toggle; do
  run_adapter "$request"
  [ "$(cat "$calls")" = "uwsm-app -- utterform --$request" ] || fail "$request did not launch Utterform: $(cat "$calls")"
done
mv "$tools/uwsm-app" "$work/uwsm-app"
run_adapter start
[ "$(cat "$calls")" = "setsid -f utterform --start" ] || fail "start without uwsm did not detach: $(cat "$calls")"
mv "$work/uwsm-app" "$tools/uwsm-app"

if run_adapter record 2>/dev/null; then
  fail "an unknown request was accepted"
fi

# --- Choosing Utterform in the menu. ---
: > "$calls"
"$prefix/bin/omarchy-install-dictation-utterform" >/dev/null
grep -qx 'omarchy-dictation-use utterform' "$calls" || fail "the backend was not chosen"
grep -qx 'omarchy-restart-hyprctl ' "$calls" || fail "Hyprland was not reloaded to bind the keys"

# --- Removing the integration. ---
mkdir -p "$XDG_CONFIG_HOME/omarchy/defaults"
printf 'utterform\n' > "$XDG_CONFIG_HOME/omarchy/defaults/dictation"
printf '#!/bin/sh\n# not ours\n' > "$work/foreign"
: > "$calls"
"$prefix/bin/omarchy-remove-dictation-utterform"
[ ! -e "$XDG_CONFIG_HOME/omarchy/defaults/dictation" ] || fail "Utterform stayed the chosen backend"
grep -qx 'omarchy-restart-hyprctl ' "$calls" || fail "Hyprland was not reloaded to release the keys"
! grep -qF "$row_id" "$menu" || fail "the menu row was not removed"
menu_rows "$menu" | grep -qx 'personal ' || fail "removing the row broke the user's menu"
for name in omarchy-dictation-utterform omarchy-install-dictation-utterform omarchy-remove-dictation-utterform; do
  [ ! -e "$prefix/bin/$name" ] || fail "$name was not removed"
done
[ -x "$prefix/bin/utterform" ] || fail "removing the integration removed Utterform"

# Another backend stays chosen, and a command that is not Utterform's stays.
install_utterform
printf 'voxtype\n' > "$XDG_CONFIG_HOME/omarchy/defaults/dictation"
cp "$work/foreign" "$prefix/bin/omarchy-dictation-utterform"
"$prefix/bin/omarchy-remove-dictation-utterform"
[ "$(cat "$XDG_CONFIG_HOME/omarchy/defaults/dictation")" = voxtype ] || fail "another backend was cleared"
[ -e "$prefix/bin/omarchy-dictation-utterform" ] || fail "a command Utterform did not install was removed"

printf 'Omarchy dictation test passed.\n'
