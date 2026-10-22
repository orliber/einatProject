#!/usr/bin/env bash
# The version on this checkout can be released (D-091): the three version numbers agree, its
# "מה חדש" lines exist, and it keeps a vault for the versions after it (F7). Run by CI on every
# PR, by the release workflow, and by `scripts/release.sh bump`. Prints the version.
set -euo pipefail
cd "$(dirname "$0")/.."

fail() { echo "::error::$*" >&2; exit 1; }

v="$(grep -m1 '^version = ' Cargo.toml | cut -d'"' -f2)"
[[ "$v" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]] || fail "Cargo.toml: the version '$v' does not look like 1.2.3"
for f in apps/desktop/src-tauri/tauri.conf.json apps/desktop/package.json; do
  w="$(grep -m1 '"version"' "$f" | cut -d'"' -f4)"
  [[ "$w" == "$v" ]] || fail "$f says $w and Cargo.toml says $v: scripts/release.sh bump $v"
done
notes="release-notes/$v.txt"
grep -q '[^[:space:]]' "$notes" 2>/dev/null \
  || fail "$notes is missing or empty: one line per change, in plain Hebrew for Einat"
test -d "tests/fixtures/vaults/$v" \
  || fail "tests/fixtures/vaults/$v is missing: DV_WRITE_VAULT_FIXTURE=$v cargo test -p dv-core upgrade::write -- --ignored"
echo "$v"
