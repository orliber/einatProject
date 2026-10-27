#!/usr/bin/env bash
# L3 (D-106): the Mac app in a bundle starts and keeps running, as after "להפעיל מחדש ולעדכן".
# Started as a program of its own (no Finder, no window to click), given some seconds, and
# stopped again. It fails when the process ends by itself in that time (the 0.8.0 kind: dyld
# stopping it before its first line, or a crash at start), and prints the crash report then.
# Usage: scripts/ci/app-starts.sh <path to .app>
set -euo pipefail
bundle="${1:?usage: app-starts.sh <bundle.app>}"
fail() { echo "::error::$*"; exit 1; }
name="$(/usr/libexec/PlistBuddy -c 'Print :CFBundleExecutable' "$bundle/Contents/Info.plist")"
exe="$bundle/Contents/MacOS/$name"
[ -x "$exe" ] || fail "no program at $exe"
log="$(mktemp)"
"$exe" >"$log" 2>&1 &
pid=$!
for i in $(seq 1 20); do
  if ! kill -0 "$pid" 2>/dev/null; then
    status=0
    wait "$pid" || status=$?
    echo "--- what it printed"; tail -c 4000 "$log" || true
    for f in $(ls -t ~/Library/Logs/DiagnosticReports/ 2>/dev/null | grep -E "^$name" | head -2); do
      echo "=== $f"; head -c 8000 ~/Library/Logs/DiagnosticReports/"$f"; echo
    done
    fail "$bundle stopped by itself ${i}s after it started (exit $status)"
  fi
  sleep 1
done
echo "$bundle started and kept running for 20 seconds"
kill "$pid" 2>/dev/null || true
for _ in $(seq 1 10); do kill -0 "$pid" 2>/dev/null || break; sleep 1; done
kill -9 "$pid" 2>/dev/null || true
wait "$pid" 2>/dev/null || true
