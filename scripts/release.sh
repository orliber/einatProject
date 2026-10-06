#!/usr/bin/env bash
# A new version for Einat's computer (D-033). Run on Or's Mac, from the repository root.
#
#   scripts/release.sh setup                  once: signing key + the public releases repository
#   scripts/release.sh bump <version>         a PR that sets the version (main is protected)
#   scripts/release.sh <version> <notes.txt>  after that PR is merged, e.g. 0.2.0 notes.txt
#
# What a release does:
#   1. the full checks, including the privacy suite (0 leaks)
#   2. checks the version on main is <version>, and pushes the tag v<version>
#   3. GitHub builds the Windows installer for that tag (.github/workflows/release.yml)
#   4. the installer is downloaded here and its checksum compared
#   5. the notice (version, notes, SHA-256) is signed HERE with the private key, and checked
#      with the program's own verification code before anything is published
#   6. installer + notice + signature go to the public releases repository; within a day
#      every installed copy offers "יש גרסה חדשה"
#
# The private key never leaves this computer. Lose it, and installed copies cannot be updated
# automatically any more (a new installer by hand fixes that); leak it, and follow
# docs/RELEASE_HE.md → "אם המפתח דלף".
set -euo pipefail

SOURCE_REPO="orliber/einatProject"
RELEASES_REPO="orliber/einat-vault-releases"
KEY="${DV_UPDATE_KEY:-$HOME/.diagnostic-vault/update-signing.pk8}"
OUT="release-out"

say() { printf '\n\033[1m%s\033[0m\n' "$*"; }
die() { printf '\n\033[31m%s\033[0m\n' "$*" >&2; exit 1; }

need() { command -v "$1" >/dev/null || die "חסר $1 (brew install $2)"; }
need gh gh
need cargo rust
need pnpm pnpm
gh auth status >/dev/null 2>&1 || die "קודם: gh auth login"

if [[ "${1:-}" == "setup" ]]; then
  say "1/2 מפתח חתימה"
  if [[ -f "$KEY" ]]; then
    echo "כבר קיים: $KEY (לא נוצר חדש)"
  else
    cargo xtask update-keygen
    echo "חשוב: לגבות את $KEY למקום בטוח ולא מחובר (למשל דיסק און קי בכספת)."
  fi
  say "2/2 ריפו ציבורי להתקנות בלבד"
  if gh repo view "$RELEASES_REPO" >/dev/null 2>&1; then
    echo "כבר קיים: $RELEASES_REPO"
  else
    gh repo create "$RELEASES_REPO" --public \
      --description "Installers for Diagnostic Vault. No code, no data: signed installers only."
    tmp="$(mktemp -d)"
    cat > "$tmp/README.md" <<'EOF'
# Diagnostic Vault – installers

Signed Windows installers only. No source code and no data of any kind live here.
Every release carries `latest.json` and `latest.json.sig` (Ed25519); the program installs a
new version only if the signature matches the key built into it and the installer's SHA-256
matches the signed notice.
EOF
    gh api -X PUT "repos/$RELEASES_REPO/contents/README.md" \
      -f message="README" -f content="$(base64 < "$tmp/README.md" | tr -d '\n')" >/dev/null
    rm -r "$tmp"
  fi
  say "סיום. עכשיו מכניסים את crates/dv-egress/update_key.pub (המפתח הציבורי בלבד) ל-main ב-PR רגיל."
  exit 0
fi

set_version() {
  perl -0pi -e "s/(\[workspace\.package\]\nversion = \")[^\"]+/\${1}$1/" Cargo.toml
  for f in apps/desktop/src-tauri/tauri.conf.json apps/desktop/package.json; do
    perl -pi -e "s/^(  \"version\": \")[^\"]+/\${1}$1/ if \$. < 6" "$f"
  done
  cargo check -q -p dv-core
}

current_versions() {
  grep -m1 '^version = ' Cargo.toml | cut -d'"' -f2
  grep -m1 '"version"' apps/desktop/src-tauri/tauri.conf.json | cut -d'"' -f4
  grep -m1 '"version"' apps/desktop/package.json | cut -d'"' -f4
}

if [[ "${1:-}" == "bump" ]]; then
  V="${2:-}"
  [[ "$V" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]] || die "שימוש: scripts/release.sh bump 0.2.0"
  [[ -z "$(git status --porcelain)" ]] || die "יש שינויים שלא נשמרו ב-git."
  git fetch -q origin main
  git checkout -q -b "release/v$V" origin/main
  set_version "$V"
  git add Cargo.toml Cargo.lock apps/desktop/src-tauri/tauri.conf.json apps/desktop/package.json
  git commit -q -m "Version $V"
  git push -q -u origin "release/v$V"
  gh pr create --repo "$SOURCE_REPO" --base main --head "release/v$V" \
    --title "Version $V" --body "מספר הגרסה $V בשלושת המקומות. אחרי המיזוג: scripts/release.sh $V notes.txt"
  say "נפתח PR לגרסה $V. אחרי שהוא ממוזג: git checkout main && git pull && scripts/release.sh $V notes.txt"
  exit 0
fi

VERSION="${1:-}"
NOTES="${2:-}"
[[ "$VERSION" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]] || die "שימוש: scripts/release.sh 0.2.0 notes.txt"
[[ -f "$NOTES" ]] || die "קובץ 'מה חדש' חסר: $NOTES (שורה לכל שינוי, בעברית פשוטה לעינת)"
[[ -f "$KEY" ]] || die "אין מפתח חתימה. קודם: scripts/release.sh setup"
grep -Eq '^[0-9a-f]{64}$' crates/dv-egress/update_key.pub || die "אין מפתח ציבורי בקוד. קודם: scripts/release.sh setup, ואז commit"
[[ -z "$(git status --porcelain)" ]] || die "יש שינויים שלא נשמרו ב-git. קודם commit או stash."
[[ "$(git rev-parse --abbrev-ref HEAD)" == "main" ]] || die "משחררים רק מ-main"
git pull --ff-only origin main
git rev-parse "v$VERSION" >/dev/null 2>&1 && die "התגית v$VERSION כבר קיימת"

say "1/6 בדיקות (כולל חבילת הדליפות)"
cargo fmt --all --check
cargo clippy --all-targets -- -D warnings
cargo test
cargo test -p dv-privacy
cargo xtask check-invariants
cargo xtask scan --all
(cd apps/desktop && pnpm install --frozen-lockfile && pnpm exec tsc --noEmit && pnpm lint && pnpm test)

say "2/6 גרסה $VERSION ותגית"
for v in $(current_versions); do
  [[ "$v" == "$VERSION" ]] || die "הגרסה ב-main היא $v ולא $VERSION. קודם: scripts/release.sh bump $VERSION (ולמזג את ה-PR)."
done
git tag -a "v$VERSION" -m "v$VERSION"
git push origin "v$VERSION"

say "3/6 GitHub בונה את המתקין ל-Windows (בערך 15 דקות)"
sleep 20
run_id="$(gh run list --repo "$SOURCE_REPO" --workflow release.yml --branch "v$VERSION" --limit 1 --json databaseId -q '.[0].databaseId')"
[[ -n "$run_id" ]] || die "לא נמצאה ריצת בנייה לתגית v$VERSION"
gh run watch "$run_id" --repo "$SOURCE_REPO" --exit-status

say "4/6 הורדה ובדיקת checksum"
rm -rf "$OUT" && mkdir -p "$OUT"
gh release download "v$VERSION" --repo "$SOURCE_REPO" --dir "$OUT" \
  -p DiagnosticVault-Setup.exe -p DiagnosticVault-Setup.exe.sha256
# The checksum file is written on Windows (CRLF): without the CR, shasum looks for the right name.
(cd "$OUT" && tr -d '\r' < DiagnosticVault-Setup.exe.sha256 | shasum -a 256 -c -)

say "5/6 חתימה ובדיקה עצמית"
cargo xtask update-notice "$VERSION" "$OUT/DiagnosticVault-Setup.exe" "$NOTES" "$OUT"

say "6/6 פרסום ב-$RELEASES_REPO"
gh release create "v$VERSION" --repo "$RELEASES_REPO" --latest \
  --title "כספת האבחון $VERSION" --notes-file "$NOTES" \
  "$OUT/DiagnosticVault-Setup.exe" "$OUT/latest.json" "$OUT/latest.json.sig"

say "הגרסה $VERSION פורסמה. אצל עינת היא תופיע כ'יש גרסה חדשה' בבדיקה היומית הבאה (או מיד, מ'הגדרות ← בדיקת עדכונים')."
