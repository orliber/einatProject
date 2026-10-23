#!/usr/bin/env bash
# Puts the FIPS 140-3 module inside the Mac app (D-047, D-091, D-101).
#
# On a Mac the validated module (aws-lc-fips-sys) can only be a dylib, named @rpath/…, never
# linked into the program. `cargo test` finds it through the build folder, so every test
# passed; the app in the disk image did not, and dyld stopped it (and its document reader)
# before its first line. This copies the dylib of the build just made and writes a config
# that puts it in Contents/Frameworks, where the program looks (build.rs adds that rpath):
#
#   cargo build --release -p dv-desktop --features fips
#   apps/desktop/src-tauri/macos/bundle-fips.sh
#   pnpm tauri build --bundles app,dmg --features fips --config src-tauri/tauri.fips.conf.json
set -euo pipefail
here="$(cd "$(dirname "$0")" && pwd)"
root="$(cd "$here/../../../.." && pwd)"

version="$(grep -A1 '^name = "aws-lc-fips-sys"$' "$root/Cargo.lock" | sed -n 's/^version = "\(.*\)"$/\1/p')"
[[ -n "$version" ]] || { echo "aws-lc-fips-sys is not in Cargo.lock" >&2; exit 1; }
pattern="libaws_lc_fips_${version//./_}*crypto*.dylib"

# The build folder of this version (an older one can still sit in the CI cache): the newest.
dylib="$(find "$root"/target/release/build -path '*/aws-lc-fips-sys-*' -name "$pattern" -type f -print0 2>/dev/null \
  | xargs -0 ls -t 2>/dev/null | head -1 || true)"
[[ -n "$dylib" ]] || { echo "no $pattern under target/release/build: build first (cargo build --release -p dv-desktop --features fips)" >&2; exit 1; }
name="$(basename "$dylib")"
echo "FIPS module: $dylib ($(shasum -a 256 "$dylib" | cut -d' ' -f1))"
otool -D "$dylib"

fips="$(cd "$here/.." && pwd)/fips"
mkdir -p "$fips"
cp -f "$dylib" "$fips/$name"

# Merged over tauri.conf.json: only the Mac frameworks change (a full path, whatever folder
# the bundler reads it from).
printf '{ "bundle": { "macOS": { "frameworks": ["%s"] } } }\n' "$fips/$name" > "$here/../tauri.fips.conf.json"
cat "$here/../tauri.fips.conf.json"
