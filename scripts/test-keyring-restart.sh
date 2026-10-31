#!/bin/sh
# Proves Utterform survives a Secret Service restart, on a private bus with a
# throwaway keyring: the real daemon and the real key are never touched.
# Needs gnome-keyring and dbus. Run from anywhere: scripts/test-keyring-restart.sh
set -eu

root=$(cd "$(dirname "$0")/.." && pwd)

if [ "${UTTERFORM_KEYRING_INNER:-}" != 1 ]; then
  sandbox=$(mktemp -d)
  trap 'rm -rf "$sandbox"' EXIT
  mkdir -m 700 "$sandbox/run"
  HOME="$sandbox" XDG_DATA_HOME="$sandbox/data" XDG_CONFIG_HOME="$sandbox/config" \
    XDG_RUNTIME_DIR="$sandbox/run" UTTERFORM_KEYRING_INNER=1 \
    CARGO_HOME="${CARGO_HOME:-$(printf %s "$PATH" | tr : '\n' | grep -m1 '/.cargo/bin$' | sed 's|/bin$||')}" \
    RUSTUP_HOME="${RUSTUP_HOME:-$(dirname "$(printf %s "$PATH" | tr : '\n' | grep -m1 '/.cargo/bin$' | sed 's|/bin$||')")/.rustup}" \
    dbus-run-session -- "$0"
  exit
fi

start_daemon() {
  printf 'throwaway' | gnome-keyring-daemon --foreground --unlock --components=secrets \
    >/dev/null 2>&1 &
  echo $! >"$XDG_RUNTIME_DIR/keyring.pid"
  attempts=0
  # Asks the bus, not the service: a call to the name itself would have dbus
  # activate a second daemon, a locked one, and the two then race for it.
  until dbus-send --session --print-reply --dest=org.freedesktop.DBus \
    /org/freedesktop/DBus org.freedesktop.DBus.NameHasOwner \
    string:org.freedesktop.secrets 2>/dev/null | grep -q 'boolean true'; do
    attempts=$((attempts + 1))
    [ "$attempts" -lt 50 ] || { echo "keyring daemon did not start" >&2; exit 1; }
    sleep 0.1
  done
}

if [ "${1:-}" = restart ]; then
  # Gone without a goodbye to its clients, as when this was found. KILL rather
  # than the original ABRT: it leaves no core dump for the desktop to report.
  kill -KILL "$(cat "$XDG_RUNTIME_DIR/keyring.pid")"
  sleep 0.3
  start_daemon
  exit
fi

start_daemon
restart=$XDG_RUNTIME_DIR/restart-keyring
printf '#!/bin/sh\nexec "%s" restart\n' "$root/scripts/test-keyring-restart.sh" >"$restart"
chmod +x "$restart"

status=0
UTTERFORM_KEYRING_RESTART=$restart cargo test \
  --manifest-path "$root/src-tauri/Cargo.toml" \
  --lib secrets:: -- --ignored --nocapture || status=$?
kill "$(cat "$XDG_RUNTIME_DIR/keyring.pid")" 2>/dev/null || true
exit "$status"
